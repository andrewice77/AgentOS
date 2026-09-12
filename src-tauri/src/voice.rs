//! Local voice I/O: Piper TTS + Whisper.cpp STT (optional binaries).

use crate::config::{data_dir, VoiceConfig};
use parking_lot::Mutex;
use regex::Regex;
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

static PLAYBACK: Mutex<Option<std::process::Child>> = Mutex::new(None);

#[derive(Debug, Clone, Serialize)]
pub struct VoiceStatus {
    pub tts_enabled: bool,
    pub stt_enabled: bool,
    pub engine: String,
    pub piper_available: bool,
    pub piper_bin: Option<String>,
    pub piper_model: Option<String>,
    pub whisper_available: bool,
    pub whisper_bin: Option<String>,
    pub whisper_model: Option<String>,
    pub audio8_available: bool,
    pub audio8_device: String,
    pub audio8_base_url: String,
    pub system_tts_hint: String,
    pub notes: Vec<String>,
}

pub fn voice_dir() -> PathBuf {
    data_dir().join("voice")
}

pub fn piper_data_dir(cfg: &VoiceConfig) -> PathBuf {
    if cfg.piper_data_dir.trim().is_empty() {
        voice_dir().join("piper")
    } else {
        PathBuf::from(cfg.piper_data_dir.trim())
    }
}

fn ensure_voice_dirs(cfg: &VoiceConfig) -> Result<(), String> {
    std::fs::create_dir_all(voice_dir()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(piper_data_dir(cfg)).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(voice_dir().join("whisper")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(voice_dir().join("tmp")).map_err(|e| e.to_string())?;
    Ok(())
}

fn which(bin: &str) -> Option<PathBuf> {
    Command::new("which")
        .arg(bin)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                None
            } else {
                Some(PathBuf::from(s))
            }
        })
}

fn resolve_piper_bin(cfg: &VoiceConfig) -> Option<String> {
    let custom = cfg.piper_bin.trim();
    if !custom.is_empty() {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p.display().to_string());
        }
    }
    let local = voice_dir().join("piper").join("piper");
    if local.exists() {
        return Some(local.display().to_string());
    }
    which("piper").map(|p| p.display().to_string())
}

fn python_piper_available() -> bool {
    Command::new("python3")
        .args(["-c", "import piper"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn resolve_whisper_bin(cfg: &VoiceConfig) -> Option<String> {
    let custom = cfg.whisper_bin.trim();
    if !custom.is_empty() {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p.display().to_string());
        }
    }
    for name in ["whisper-cli", "whisper-cpp", "main"] {
        let local = voice_dir().join("whisper").join(name);
        if local.exists() {
            return Some(local.display().to_string());
        }
        if let Some(p) = which(name) {
            return Some(p.display().to_string());
        }
    }
    None
}

fn resolve_whisper_model(cfg: &VoiceConfig) -> Option<PathBuf> {
    let custom = cfg.whisper_model.trim();
    if !custom.is_empty() {
        let p = PathBuf::from(custom);
        if p.exists() {
            return Some(p);
        }
    }
    let dir = voice_dir().join("whisper");
    for name in [
        "ggml-base.bin",
        "ggml-small.bin",
        "ggml-tiny.bin",
        "ggml-base-q5_1.bin",
    ] {
        let p = dir.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn resolve_piper_model(cfg: &VoiceConfig) -> Option<PathBuf> {
    let data = piper_data_dir(cfg);
    let voice = cfg.voice.trim();
    if voice.is_empty() {
        return None;
    }
    // Accept absolute/relative path to .onnx
    let as_path = PathBuf::from(voice);
    if as_path.extension().and_then(|e| e.to_str()) == Some("onnx") && as_path.exists() {
        return Some(as_path);
    }
    let candidates = [
        data.join(format!("{voice}.onnx")),
        data.join(voice).with_extension("onnx"),
        data.join(format!("{voice}/{voice}.onnx")),
    ];
    candidates.into_iter().find(|p| p.exists())
}

fn audio8_runtime_dir(cfg: &VoiceConfig) -> PathBuf {
    let custom = cfg.audio8_runtime_dir.trim();
    if !custom.is_empty() {
        return PathBuf::from(custom);
    }
    voice_dir().join("audio8").join("Audio8_TTS").join("onnx_runtime")
}

fn audio8_http_reachable(base_url: &str) -> bool {
    let url = format!("{}/", base_url.trim_end_matches('/'));
    Command::new("curl")
        .args(["-sf", "--max-time", "1", "-o", "/dev/null", &url])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn audio8_gpu_script() -> Option<PathBuf> {
    // Prefer repo script next to the binary workspace, then ~/.agentos
    let candidates = [
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../scripts/audio8_infer_gpu.py"),
        voice_dir().join("audio8_infer_gpu.py"),
        data_dir().join("scripts/audio8_infer_gpu.py"),
    ];
    candidates.into_iter().find(|p| p.exists())
}

pub fn status(cfg: &VoiceConfig) -> VoiceStatus {
    let mut notes = Vec::new();
    let piper_bin = resolve_piper_bin(cfg);
    let py = python_piper_available();
    let piper_available = piper_bin.is_some() || py;
    if !piper_available {
        notes.push(
            "Piper non trovato. Installa con: pip install piper-tts  (o metti il binario in ~/.agentos/voice/piper/)."
                .into(),
        );
    }
    let piper_model = resolve_piper_model(cfg).map(|p| p.display().to_string());
    if piper_available && piper_model.is_none() && cfg.engine == "piper" {
        notes.push(format!(
            "Voce Piper «{}» non trovata in {}. Scarica con: python3 -m piper.download_voices {} --data-dir {}",
            cfg.voice,
            piper_data_dir(cfg).display(),
            cfg.voice,
            piper_data_dir(cfg).display()
        ));
    }
    let whisper_bin = resolve_whisper_bin(cfg);
    let whisper_model = resolve_whisper_model(cfg).map(|p| p.display().to_string());
    let whisper_available = whisper_bin.is_some() && whisper_model.is_some();
    if cfg.stt_enabled && !whisper_available {
        notes.push(
            "Whisper non pronto: serve binario (whisper-cli) e modello ggml in ~/.agentos/voice/whisper/."
                .into(),
        );
    }

    let audio8_device = if cfg.audio8_device.trim().is_empty() {
        "cpu".into()
    } else {
        cfg.audio8_device.trim().to_lowercase()
    };
    let audio8_base_url = if cfg.audio8_base_url.trim().is_empty() {
        "http://127.0.0.1:8024".into()
    } else {
        cfg.audio8_base_url.trim().to_string()
    };
    let runtime = audio8_runtime_dir(cfg);
    let audio8_available = if audio8_device == "cuda" || audio8_device == "gpu" {
        audio8_gpu_script().is_some()
    } else {
        audio8_http_reachable(&audio8_base_url) || runtime.join("run_infer.sh").exists()
    };
    if cfg.engine == "audio8" && !audio8_available {
        if audio8_device == "cuda" || audio8_device == "gpu" {
            notes.push(
                "Audio8 CUDA: manca scripts/audio8_infer_gpu.py o le dipendenze torch/transformers."
                    .into(),
            );
        } else {
            notes.push(format!(
                "Audio8 CPU: avvia il servizio ONNX (`./scripts/setup-audio8.sh` poi start_server) su {} — lascia la GPU a Ollama.",
                audio8_base_url
            ));
        }
    } else if cfg.engine == "audio8" && audio8_device == "cpu" {
        notes.push(
            "Audio8 su CPU (ONNX): Ollama resta sulla GPU. Per usarlo sulla GPU imposta audio8_device=cuda (attenzione alla VRAM)."
                .into(),
        );
    }

    VoiceStatus {
        tts_enabled: cfg.enabled,
        stt_enabled: cfg.stt_enabled,
        engine: cfg.engine.clone(),
        piper_available,
        piper_bin,
        piper_model,
        whisper_available,
        whisper_bin,
        whisper_model,
        audio8_available,
        audio8_device,
        audio8_base_url,
        system_tts_hint: "Motore «system» usa Web Speech API del webview (voci OS).".into(),
        notes,
    }
}

const SPEECH_TEXT_LIMIT: usize = 1800;

fn is_speakable_inline_code(code: &str) -> bool {
    let c = code.trim();
    if c.is_empty() || c.chars().count() > 48 {
        return false;
    }
    if c.contains(['{', '}', '[', ']', '(', ')', ';', '=', '<', '>', '/', '\\', '|']) {
        return false;
    }
    for starter in [
        "import", "export", "function", "const", "let", "var", "class", "def", "return", "if",
        "else", "for", "while", "async", "await",
    ] {
        if c.starts_with(starter) {
            return false;
        }
    }
    c.chars()
        .all(|ch| ch.is_alphanumeric() || matches!(ch, ' ' | '.' | '_' | '-'))
}

fn strip_code_fences_line_wise(text: &str) -> String {
    let mut in_code = false;
    let mut kept = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        kept.push(line);
    }
    kept.join("\n")
}

fn apply_phonetic(text: &str) -> String {
    let pairs: &[(&str, &str)] = &[
        (r"(?i)\bAgentOS\b", "Eigent os"),
        (r"(?i)\bOllama\b", "Olàma"),
        (r"(?i)\bWhisper\b", "Uìsper"),
        (r"(?i)\bAudio8\b", "Àudio otto"),
        (r"\bTTS\b", "ti ti esse"),
        (r"\bSTT\b", "esse ti ti"),
        (r"\bMCP\b", "eme ci pi"),
        (r"\bAPI\b", "a pi ai"),
        (r"\bGPU\b", "gi pi u"),
        (r"\bCPU\b", "ci pi u"),
        (r"(?i)\bONNX\b", "on ix"),
        (r"(?i)\bHTTP\b", "acce ti ti pi"),
        (r"\bURL\b", "u ar el"),
    ];
    let mut out = text.to_string();
    for (pattern, replacement) in pairs {
        if let Ok(re) = Regex::new(pattern) {
            out = re.replace_all(&out, *replacement).into_owned();
        }
    }
    out
}

fn regex_replace(text: &str, pattern: &str, replacement: &str) -> String {
    Regex::new(pattern)
        .map(|re| re.replace_all(text, replacement).into_owned())
        .unwrap_or_else(|_| text.to_string())
}

fn sanitize_for_speech(text: &str) -> String {
    let mut out = regex_replace(text, r"<[^>]+>", " ");
    out = strip_code_fences_line_wise(&out);
    out = regex_replace(&out, r"```[\s\S]*?```", " ");
    out = regex_replace(&out, r"~~~[\s\S]*?~~~", " ");

    out = regex_replace(&out, r"!\[([^\]]*)\]\([^)]+\)", "$1");
    out = regex_replace(&out, r"\[([^\]]+)\]\([^)]+\)", "$1");
    out = regex_replace(&out, r"\[([^\]]+)\]\[[^\]]*\]", "$1");
    out = regex_replace(&out, r"<https?://[^>]+>", " ");

    if let Ok(re) = Regex::new(r"`([^`]+)`") {
        out = re
            .replace_all(&out, |caps: &regex::Captures| {
                let code = caps.get(1).map(|m| m.as_str()).unwrap_or("");
                if is_speakable_inline_code(code) {
                    code.trim().to_string()
                } else {
                    " ".to_string()
                }
            })
            .into_owned();
    }

    out = regex_replace(&out, r"(?m)^#{1,6}\s+", "");
    out = regex_replace(&out, r"(?m)^>\s?", "");
    out = regex_replace(&out, r"(?m)^[\s]*[-*+]\s+", "");
    out = regex_replace(&out, r"(?m)^[\s]*\d+\.\s+", "");
    out = regex_replace(&out, r"(?m)^[\s]*([-*_]){3,}[\s]*$", " ");

    if let Ok(re) = Regex::new(r"(?m)^[^\n]*\|[^\n]*$") {
        out = re
            .replace_all(&out, |caps: &regex::Captures| {
                let line = caps.get(0).map(|m| m.as_str()).unwrap_or("");
                if Regex::new(r"^\s*\|?[\s:-]+\|[\s|:-]+\|?\s*$")
                    .map(|r| r.is_match(line))
                    .unwrap_or(false)
                {
                    " ".to_string()
                } else {
                    line.replace('|', ", ")
                        .trim_matches(|c: char| c.is_whitespace() || c == '|')
                        .to_string()
                }
            })
            .into_owned();
    }

    out = regex_replace(&out, r"\*\*\*([^*]+)\*\*\*", "$1");
    out = regex_replace(&out, r"___([^_]+)___", "$1");
    out = regex_replace(&out, r"\*\*([^*]+)\*\*", "$1");
    out = regex_replace(&out, r"__([^_]+)__", "$1");
    out = regex_replace(&out, r"\*([^*\n]+)\*", "$1");
    out = regex_replace(&out, r"_([^_\n]+)_", "$1");
    out = regex_replace(&out, r"~~([^~]+)~~", "$1");

    out = regex_replace(&out, r"[#*`~]", " ");
    out = apply_phonetic(&out);
    out = regex_replace(&out, r"\s+", " ");
    out.trim().chars().take(SPEECH_TEXT_LIMIT).collect()
}

fn tmp_wav(prefix: &str) -> PathBuf {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    voice_dir().join("tmp").join(format!("{prefix}-{n}.wav"))
}

pub fn stop_speech() -> Result<(), String> {
    let mut guard = PLAYBACK.lock();
    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

fn play_wav(path: &Path) -> Result<(), String> {
    stop_speech()?;
    let player = if which("paplay").is_some() {
        "paplay"
    } else if which("aplay").is_some() {
        "aplay"
    } else {
        return Err("Nessun player audio (paplay/aplay)".into());
    };
    let child = Command::new(player)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("play: {e}"))?;
    *PLAYBACK.lock() = Some(child);
    // Wait for playback end (best-effort)
    if let Some(mut child) = PLAYBACK.lock().take() {
        let _ = child.wait();
    }
    let _ = std::fs::remove_file(path);
    Ok(())
}

pub async fn speak_piper(cfg: VoiceConfig, text: String) -> Result<(), String> {
    let clean = sanitize_for_speech(&text);
    if clean.is_empty() {
        return Ok(());
    }
    ensure_voice_dirs(&cfg)?;
    let out = tmp_wav("tts");
    let model = resolve_piper_model(&cfg).ok_or_else(|| {
        format!(
            "Modello Piper «{}» assente. Vedi Settings → Voce.",
            cfg.voice
        )
    })?;
    let data_dir = piper_data_dir(&cfg);

    let result = tokio::task::spawn_blocking(move || {
        if let Some(bin) = resolve_piper_bin(&cfg) {
            let mut child = Command::new(&bin)
                .args([
                    "--model",
                    &model.display().to_string(),
                    "--output_file",
                    &out.display().to_string(),
                ])
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("piper spawn: {e}"))?;
            if let Some(mut stdin) = child.stdin.take() {
                stdin
                    .write_all(clean.as_bytes())
                    .map_err(|e| format!("piper stdin: {e}"))?;
            }
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            if !output.status.success() {
                let err = String::from_utf8_lossy(&output.stderr);
                return Err(format!("piper failed: {err}"));
            }
        } else if python_piper_available() {
            let voice_name = cfg.voice.trim().to_string();
            let child = Command::new("python3")
                .args([
                    "-m",
                    "piper",
                    "-m",
                    &voice_name,
                    "-f",
                    &out.display().to_string(),
                    "--data-dir",
                    &data_dir.display().to_string(),
                    "--",
                    &clean,
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|e| format!("python piper: {e}"))?;
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            if !output.status.success() {
                // Fallback: stdin pipe style
                let mut child = Command::new("python3")
                    .args([
                        "-m",
                        "piper",
                        "--model",
                        &voice_name,
                        "--output_file",
                        &out.display().to_string(),
                        "--data-dir",
                        &data_dir.display().to_string(),
                    ])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| format!("python piper: {e}"))?;
                if let Some(mut stdin) = child.stdin.take() {
                    stdin
                        .write_all(clean.as_bytes())
                        .map_err(|e| format!("piper stdin: {e}"))?;
                }
                let output = child.wait_with_output().map_err(|e| e.to_string())?;
                if !output.status.success() {
                    let err = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("python piper failed: {err}"));
                }
            }
        } else {
            return Err("Piper non disponibile".into());
        }
        if !out.exists() {
            return Err("Piper non ha prodotto il file WAV".into());
        }
        play_wav(&out)
    })
    .await
    .map_err(|e| e.to_string())?;

    result
}

pub async fn speak_audio8(cfg: VoiceConfig, text: String) -> Result<(), String> {
    let clean = sanitize_for_speech(&text);
    if clean.is_empty() {
        return Ok(());
    }
    ensure_voice_dirs(&cfg)?;
    let device = cfg.audio8_device.trim().to_lowercase();
    if device == "cuda" || device == "gpu" {
        speak_audio8_cuda(cfg, clean).await
    } else {
        // Prefer HTTP service (warm model on CPU). Fallback: one-shot CLI.
        match speak_audio8_http(&cfg, &clean).await {
            Ok(()) => Ok(()),
            Err(http_err) => {
                match speak_audio8_cli(&cfg, &clean) {
                    Ok(()) => Ok(()),
                    Err(cli_err) => Err(format!(
                        "Audio8 CPU fallito.\nHTTP: {http_err}\nCLI: {cli_err}\n\
                         Setup: ./scripts/setup-audio8.sh && avvia start_server.sh"
                    )),
                }
            }
        }
    }
}

async fn speak_audio8_http(cfg: &VoiceConfig, text: &str) -> Result<(), String> {
    let base = if cfg.audio8_base_url.trim().is_empty() {
        "http://127.0.0.1:8024".to_string()
    } else {
        cfg.audio8_base_url.trim().trim_end_matches('/').to_string()
    };
    let voice = if !cfg.audio8_voice.trim().is_empty() {
        cfg.audio8_voice.trim().to_string()
    } else if !cfg.voice.trim().is_empty() && !cfg.voice.contains("it_IT") {
        cfg.voice.trim().to_string()
    } else {
        "default".to_string()
    };
    let out = tmp_wav("audio8");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(180))
        .build()
        .map_err(|e| e.to_string())?;

    // OpenAI-compatible first
    let openai_url = format!("{base}/v1/audio/speech");
    let openai_body = serde_json::json!({
        "model": "arktts",
        "input": text,
        "voice": voice,
        "response_format": "wav",
    });
    let resp = client.post(&openai_url).json(&openai_body).send().await;
    let bytes = match resp {
        Ok(r) if r.status().is_success() => r.bytes().await.map_err(|e| e.to_string())?,
        Ok(r) => {
            let status = r.status();
            let api_url = format!("{base}/api/tts");
            let api_body = serde_json::json!({
                "text": text,
                "voice_name": voice,
                "max_new_tokens": audio8_token_budget(text),
            });
            let r2 = client
                .post(&api_url)
                .json(&api_body)
                .send()
                .await
                .map_err(|e| format!("audio8 /api/tts: {e}"))?;
            if !r2.status().is_success() {
                let body = r2.text().await.unwrap_or_default();
                return Err(format!(
                    "audio8 HTTP {status} / openai e /api/tts falliti: {body}"
                ));
            }
            r2.bytes().await.map_err(|e| e.to_string())?
        }
        Err(e) => return Err(format!("audio8 unreachable ({base}): {e}")),
    };

    if bytes.len() < 64 {
        return Err("audio8 ha restituito una risposta troppo corta".into());
    }
    tokio::fs::write(&out, &bytes)
        .await
        .map_err(|e| format!("write wav: {e}"))?;
    tokio::task::spawn_blocking(move || play_wav(&out))
        .await
        .map_err(|e| e.to_string())?
}

fn speak_audio8_cli(cfg: &VoiceConfig, text: &str) -> Result<(), String> {
    let runtime = audio8_runtime_dir(cfg);
    let infer = runtime.join("run_infer.sh");
    if !infer.exists() {
        return Err(format!(
            "run_infer.sh assente in {}",
            runtime.display()
        ));
    }
    let voice = if !cfg.audio8_voice.trim().is_empty() {
        cfg.audio8_voice.trim().to_string()
    } else {
        "default".to_string()
    };
    let out = tmp_wav("audio8-cli");
    let status = Command::new("bash")
        .arg(&infer)
        .args([
            "--text",
            text,
            "--voice",
            &voice,
            "--max-new-tokens",
            "512",
            "--output",
            &out.display().to_string(),
        ])
        .current_dir(&runtime)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .status()
        .map_err(|e| format!("audio8 cli: {e}"))?;
    if !status.success() {
        return Err("run_infer.sh failed".into());
    }
    if !out.exists() {
        return Err("Audio8 CLI non ha prodotto WAV".into());
    }
    play_wav(&out)
}

async fn speak_audio8_cuda(cfg: VoiceConfig, text: String) -> Result<(), String> {
    let script = audio8_gpu_script().ok_or_else(|| {
        "Script GPU Audio8 non trovato (scripts/audio8_infer_gpu.py)".to_string()
    })?;
    let out = tmp_wav("audio8-gpu");
    let model_id = if cfg.audio8_model_id.trim().is_empty() {
        "Audio8/Audio8-TTS-Preview-0.6b".to_string()
    } else {
        cfg.audio8_model_id.trim().to_string()
    };
    let out_s = out.display().to_string();
    let result = tokio::task::spawn_blocking(move || {
        let output = Command::new("python3")
            .arg(&script)
            .args([
                "--text",
                &text,
                "--out",
                &out_s,
                "--device",
                "cuda",
                "--model",
                &model_id,
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("audio8 gpu spawn: {e}"))?;
        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(format!("audio8 gpu failed: {err}"));
        }
        if !Path::new(&out_s).exists() {
            return Err("Audio8 GPU non ha prodotto WAV".into());
        }
        play_wav(Path::new(&out_s))
    })
    .await
    .map_err(|e| e.to_string())?;
    result
}

pub async fn record_and_transcribe(cfg: VoiceConfig, seconds: u32) -> Result<String, String> {
    if !cfg.stt_enabled {
        return Err("Input vocale disabilitato in Settings".into());
    }
    ensure_voice_dirs(&cfg)?;
    let bin = resolve_whisper_bin(&cfg).ok_or("Whisper binario non trovato")?;
    let model = resolve_whisper_model(&cfg).ok_or("Modello Whisper non trovato")?;
    let secs = seconds.clamp(1, 30);
    let wav = tmp_wav("stt");
    let wav2 = wav.clone();
    let model2 = model.clone();
    let bin2 = bin.clone();

    tokio::task::spawn_blocking(move || {
        // Prefer arecord (ALSA); fall back to sox rec
        let record = if which("arecord").is_some() {
            Command::new("arecord")
                .args([
                    "-f",
                    "S16_LE",
                    "-r",
                    "16000",
                    "-c",
                    "1",
                    "-d",
                    &secs.to_string(),
                    &wav.display().to_string(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .status()
        } else if which("rec").is_some() {
            Command::new("rec")
                .args([
                    "-q",
                    "-r",
                    "16000",
                    "-c",
                    "1",
                    &wav.display().to_string(),
                    "trim",
                    "0",
                    &secs.to_string(),
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .status()
        } else {
            return Err("Serve arecord o sox (rec) per catturare il microfono".into());
        };
        let status = record.map_err(|e| format!("record: {e}"))?;
        if !status.success() {
            return Err("Registrazione microfono fallita".into());
        }
        if !wav2.exists() {
            return Err("File audio non creato".into());
        }

        // whisper.cpp CLI variants
        let attempts = [
            vec![
                "-m".into(),
                model2.display().to_string(),
                "-f".into(),
                wav2.display().to_string(),
                "-l".into(),
                "it".into(),
                "-nt".into(),
            ],
            vec![
                "-m".into(),
                model2.display().to_string(),
                "-f".into(),
                wav2.display().to_string(),
                "-l".into(),
                "it".into(),
            ],
        ];
        let mut last_err = String::new();
        for args in attempts {
            let output = Command::new(&bin2)
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();
            match output {
                Ok(o) if o.status.success() => {
                    let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    let _ = std::fs::remove_file(&wav2);
                    if text.is_empty() {
                        return Err("Whisper: trascrizione vuota".into());
                    }
                    return Ok(text);
                }
                Ok(o) => {
                    last_err = String::from_utf8_lossy(&o.stderr).trim().to_string();
                }
                Err(e) => last_err = e.to_string(),
            }
        }
        let _ = std::fs::remove_file(&wav2);
        Err(format!("whisper failed: {last_err}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

fn audio8_token_budget(text: &str) -> u32 {
    let n = text.chars().count();
    if n < 40 {
        96
    } else if n < 120 {
        192
    } else if n < 280 {
        320
    } else {
        512
    }
}

pub async fn speak_engine(cfg: VoiceConfig, text: String) -> Result<(), String> {
    match cfg.engine.as_str() {
        "piper" => speak_piper(cfg, text).await,
        "audio8" => speak_audio8(cfg, text).await,
        other => Err(format!("Motore TTS non supportato in pipeline: {other}")),
    }
}

/// Incremental sentence splitter for streaming TTS.
#[derive(Default)]
pub struct SentenceBuffer {
    buf: String,
}

impl SentenceBuffer {
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buf.push_str(chunk);
        let mut out = Vec::new();
        while let Some((sent, rest)) = take_sentence(&self.buf) {
            let t = sent.trim().to_string();
            self.buf = rest;
            if t.chars().count() >= 8 {
                out.push(t);
            } else if !t.is_empty() {
                self.buf = format!("{t}{}", self.buf);
                break;
            }
        }
        if self.buf.chars().count() > 220 {
            if let Some((idx, _)) = self.buf.char_indices().find(|(i, c)| {
                *i > 80 && (*c == ',' || *c == ';' || *c == ':' || c.is_whitespace())
            }) {
                let (head, tail) = self.buf.split_at(idx + 1);
                let t = head.trim().to_string();
                self.buf = tail.to_string();
                if t.chars().count() >= 8 {
                    out.push(t);
                }
            }
        }
        out
    }

    pub fn flush(&mut self) -> Option<String> {
        let t = sanitize_for_speech(&self.buf);
        self.buf.clear();
        if t.is_empty() {
            None
        } else {
            Some(t)
        }
    }
}

fn take_sentence(s: &str) -> Option<(String, String)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let ch = s[i..].chars().next()?;
        let len = ch.len_utf8();
        let is_end = matches!(ch, '.' | '!' | '?' | '…' | '。' | '！' | '？');
        if is_end {
            let mut j = i + len;
            while j < bytes.len() {
                let c = s[j..].chars().next()?;
                if matches!(c, '"' | '\'' | '»' | '”' | '’' | ')' | ']' | '}') {
                    j += c.len_utf8();
                } else {
                    break;
                }
            }
            if j >= bytes.len() {
                return Some((s[..j].to_string(), String::new()));
            }
            let next = s[j..].chars().next()?;
            if next.is_whitespace() {
                while j < bytes.len() && s[j..].chars().next().is_some_and(|c| c.is_whitespace()) {
                    j += s[j..].chars().next().unwrap().len_utf8();
                }
                return Some((s[..j].to_string(), s[j..].to_string()));
            }
        }
        i += len;
    }
    None
}

enum SpeechJob {
    Speak {
        cfg: VoiceConfig,
        text: String,
        generation: usize,
    },
    StopPlayback,
}

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter};
use tokio::sync::mpsc::UnboundedSender;

static SPEECH_TX: OnceLock<UnboundedSender<SpeechJob>> = OnceLock::new();
static PENDING: AtomicUsize = AtomicUsize::new(0);
static GENERATION: AtomicUsize = AtomicUsize::new(0);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn set_app_handle(app: AppHandle) {
    let _ = APP_HANDLE.set(app);
}

pub fn ensure_speech_worker() {
    SPEECH_TX.get_or_init(|| {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SpeechJob>();
        tauri::async_runtime::spawn(async move {
            while let Some(job) = rx.recv().await {
                match job {
                    SpeechJob::StopPlayback => {
                        let _ = stop_speech();
                    }
                    SpeechJob::Speak {
                        cfg,
                        text,
                        generation,
                    } => {
                        if generation != GENERATION.load(Ordering::SeqCst) {
                            let left = PENDING.fetch_sub(1, Ordering::SeqCst);
                            if left <= 1 {
                                emit_idle();
                            }
                            continue;
                        }
                        let clean = sanitize_for_speech(&text);
                        if !clean.is_empty() {
                            let _ = speak_engine(cfg, clean).await;
                        }
                        let left = PENDING.fetch_sub(1, Ordering::SeqCst);
                        if left <= 1 {
                            emit_idle();
                        }
                    }
                }
            }
        });
        tx
    });
}

fn emit_idle() {
    if let Some(app) = APP_HANDLE.get() {
        let _ = app.emit("avatar-state", "idle");
        let _ = app.emit("tts-queue-empty", true);
    }
}

pub fn begin_reply_speech() -> usize {
    ensure_speech_worker();
    let gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(tx) = SPEECH_TX.get() {
        let _ = tx.send(SpeechJob::StopPlayback);
    }
    let _ = stop_speech();
    gen
}

pub fn enqueue_sentence(cfg: &VoiceConfig, text: &str, generation: usize) {
    if generation != GENERATION.load(Ordering::SeqCst) {
        return;
    }
    let clean = sanitize_for_speech(text);
    if clean.is_empty() {
        return;
    }
    ensure_speech_worker();
    if let Some(tx) = SPEECH_TX.get() {
        PENDING.fetch_add(1, Ordering::SeqCst);
        let _ = tx.send(SpeechJob::Speak {
            cfg: cfg.clone(),
            text: clean,
            generation,
        });
    }
}

pub fn local_tts_enabled(cfg: &VoiceConfig) -> bool {
    cfg.enabled
        && cfg.auto_read_replies
        && cfg.stream_sentences
        && (cfg.engine == "piper" || cfg.engine == "audio8")
}

#[cfg(test)]
mod tests {
    use super::sanitize_for_speech;

    #[test]
    fn strips_markdown_formatting() {
        let out = sanitize_for_speech("**Grassetto** e *corsivo* con `task`.");
        assert_eq!(out, "Grassetto e corsivo con task.");
    }

    #[test]
    fn skips_code_blocks_and_keeps_links() {
        let out = sanitize_for_speech(
            "Vedi [impostazioni](https://x.test).\n```python\ndef foo():\n    pass\n```\nFine.",
        );
        assert_eq!(out, "Vedi impostazioni. Fine.");
    }

    #[test]
    fn applies_phonetic_replacements() {
        let out = sanitize_for_speech("AgentOS usa Ollama e Whisper.");
        assert_eq!(out, "Eigent os usa Olàma e Uìsper.");
    }
}
