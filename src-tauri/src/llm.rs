use crate::config::AppConfig;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmStatus {
    pub provider: String,
    pub model: String,
    pub badge: String,
    pub ollama_reachable: bool,
}

pub async fn check_ollama(base_url: &str) -> bool {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(5))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

pub async fn list_ollama_models(base_url: &str) -> Result<Vec<String>, String> {
    let url = format!("{}/api/tags", base_url.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let models = body
        .get("models")
        .and_then(|m| m.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(|n| n.as_str()).map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    Ok(models)
}

fn api_key(provider: &str) -> Result<Option<String>, String> {
    if provider == "ollama" {
        return Ok(None);
    }
    let entry = keyring::Entry::new("agentos", provider).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

pub fn set_api_key(provider: &str, key: &str) -> Result<(), String> {
    let entry = keyring::Entry::new("agentos", provider).map_err(|e| e.to_string())?;
    entry.set_password(key).map_err(|e| e.to_string())
}

pub fn clear_api_key(provider: &str) -> Result<(), String> {
    let entry = keyring::Entry::new("agentos", provider).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Non-streaming completion for simple agent turns / tool loops.
pub async fn complete(
    cfg: &AppConfig,
    messages: &[ChatMessage],
) -> Result<String, String> {
    complete_with_temp(cfg, messages, 0.7).await
}

/// Lower temperature for MCP tool loops (local models stay on-format better).
pub async fn complete_tools(
    cfg: &AppConfig,
    messages: &[ChatMessage],
) -> Result<String, String> {
    complete_with_temp(cfg, messages, 0.2).await
}

async fn complete_with_temp(
    cfg: &AppConfig,
    messages: &[ChatMessage],
    temperature: f32,
) -> Result<String, String> {
    match cfg.provider.as_str() {
        "ollama" => ollama_chat(cfg, messages, temperature).await,
        "openai" => openai_compatible_chat(cfg, messages, "openai").await,
        "anthropic" => anthropic_chat(cfg, messages).await,
        other => Err(format!("unsupported provider: {other}")),
    }
}

async fn ollama_chat(
    cfg: &AppConfig,
    messages: &[ChatMessage],
    temperature: f32,
) -> Result<String, String> {
    let url = format!(
        "{}/api/chat",
        cfg.ollama.base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": cfg.model,
        "stream": false,
        // Qwen3.5 otherwise spends a long time in hidden "thinking" with empty content
        "think": false,
        "messages": messages,
        "options": {
            "num_ctx": 16384,
            "num_predict": 2048,
            "temperature": temperature
        }
    });
    let resp = reqwest::Client::new()
        .post(url)
        .timeout(Duration::from_secs(120))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Ollama error: {}", resp.text().await.unwrap_or_default()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    v.pointer("/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Ollama response missing content".into())
}

async fn openai_compatible_chat(
    cfg: &AppConfig,
    messages: &[ChatMessage],
    provider: &str,
) -> Result<String, String> {
    let key = api_key(provider)?
        .ok_or_else(|| format!("No API key configured for {provider}. Set it in Settings."))?;
    let base = if provider == "openai" {
        cfg.openai.base_url.trim_end_matches('/')
    } else {
        return Err("internal provider mismatch".into());
    };
    let url = format!("{base}/chat/completions");
    let body = json!({
        "model": cfg.model,
        "messages": messages,
        "stream": false,
    });
    let resp = reqwest::Client::new()
        .post(url)
        .bearer_auth(key)
        .timeout(Duration::from_secs(120))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{provider} error: {}", resp.text().await.unwrap_or_default()));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    v.pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("{provider} response missing content"))
}

async fn anthropic_chat(cfg: &AppConfig, messages: &[ChatMessage]) -> Result<String, String> {
    let key = api_key("anthropic")?
        .ok_or_else(|| "No API key configured for anthropic. Set it in Settings.".to_string())?;
    let system = messages
        .iter()
        .filter(|m| m.role == "system")
        .map(|m| m.content.clone())
        .collect::<Vec<_>>()
        .join("\n\n");
    let filtered: Vec<_> = messages
        .iter()
        .filter(|m| m.role != "system")
        .map(|m| json!({"role": m.role, "content": m.content}))
        .collect();
    let url = format!(
        "{}/v1/messages",
        cfg.anthropic.base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": cfg.model,
        "max_tokens": 2048,
        "system": system,
        "messages": filtered,
    });
    let resp = reqwest::Client::new()
        .post(url)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .timeout(Duration::from_secs(120))
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!(
            "anthropic error: {}",
            resp.text().await.unwrap_or_default()
        ));
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    v.pointer("/content/0/text")
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "anthropic response missing content".into())
}

/// Stream tokens via callback. Ollama streams content only (ignores thinking).
/// Falls back to non-streaming complete() if the stream yields no visible content.
pub async fn stream_chat<F>(
    cfg: &AppConfig,
    messages: &[ChatMessage],
    mut on_token: F,
) -> Result<String, String>
where
    F: FnMut(String),
{
    if cfg.provider != "ollama" {
        let full = complete(cfg, messages).await?;
        on_token(full.clone());
        return Ok(full);
    }

    match stream_ollama(cfg, messages, &mut on_token).await {
        Ok(full) if !full.trim().is_empty() => Ok(full),
        Ok(_) => {
            crate::debuglog::warn(None, "ollama:stream_empty — falling back to complete()");
            let full = complete(cfg, messages).await?;
            on_token(full.clone());
            Ok(full)
        }
        Err(e) => {
            crate::debuglog::warn(None, format!("ollama:stream_err {e} — falling back to complete()"));
            let full = complete(cfg, messages).await?;
            on_token(full.clone());
            Ok(full)
        }
    }
}

async fn stream_ollama<F>(
    cfg: &AppConfig,
    messages: &[ChatMessage],
    on_token: &mut F,
) -> Result<String, String>
where
    F: FnMut(String),
{
    let url = format!(
        "{}/api/chat",
        cfg.ollama.base_url.trim_end_matches('/')
    );
    let body = json!({
        "model": cfg.model,
        "stream": true,
        "think": false,
        "messages": messages,
        "options": {
            "num_ctx": 8192,
            "temperature": 0.7,
            "num_predict": 1024
        }
    });
    crate::debuglog::info(None, format!("ollama:stream_post {}", url));
    let resp = reqwest::Client::new()
        .post(url)
        .timeout(Duration::from_secs(90))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama stream failed: {e}"))?;
    crate::debuglog::info(None, format!("ollama:stream_status {}", resp.status()));
    if !resp.status().is_success() {
        return Err(format!(
            "Ollama error: {}",
            resp.text().await.unwrap_or_default()
        ));
    }

    let mut full = String::new();
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();
    let mut last_content = Instant::now();
    let idle = Duration::from_secs(45);

    while let Some(chunk) = stream.next().await {
        if last_content.elapsed() > idle && full.is_empty() {
            return Err("stream idle senza content (possibile thinking)".into());
        }
        let chunk = chunk.map_err(|e| e.to_string())?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].trim().to_string();
            buffer = buffer[pos + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) {
                // Only visible content — never stream thinking into the UI
                if let Some(token) = v
                    .pointer("/message/content")
                    .and_then(|c| c.as_str())
                    .filter(|s| !s.is_empty())
                {
                    full.push_str(token);
                    last_content = Instant::now();
                    on_token(token.to_string());
                }
            }
        }
    }
    Ok(full)
}
