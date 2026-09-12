use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: String,
    pub model: String,
    pub ollama: OllamaConfig,
    pub openai: CloudConfig,
    pub anthropic: CloudConfig,
    pub memory: MemoryConfig,
    pub mcp_servers: Vec<McpServerConfig>,
    pub permissions: PermissionsConfig,
    /// Avatar skin id: "buddy" | "orb" (Lumina) | "familiar"
    #[serde(default = "default_avatar")]
    pub avatar: String,
    /// If true, the floating avatar slowly wanders across the monitor.
    #[serde(default)]
    pub avatar_roam: bool,
    #[serde(default)]
    pub voice: VoiceConfig,
    #[serde(default)]
    pub briefing: BriefingConfig,
    #[serde(default)]
    pub skills: SkillsConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    /// Theme preset: midnight | slate | light | ocean | warm
    #[serde(default = "default_ui_theme")]
    pub theme: String,
    /// Accent color: blue | violet | amber | rose | teal
    #[serde(default = "default_ui_accent")]
    pub accent: String,
    /// comfortable | compact
    #[serde(default = "default_ui_density")]
    pub density: String,
    /// Content layout: single | split
    #[serde(default = "default_ui_content_layout")]
    pub content_layout: String,
}

fn default_ui_theme() -> String {
    "midnight".into()
}
fn default_ui_accent() -> String {
    "blue".into()
}
fn default_ui_density() -> String {
    "comfortable".into()
}
fn default_ui_content_layout() -> String {
    "split".into()
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: default_ui_theme(),
            accent: default_ui_accent(),
            density: default_ui_density(),
            content_layout: default_ui_content_layout(),
        }
    }
}

fn default_avatar() -> String {
    "buddy".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// Master TTS switch
    #[serde(default)]
    pub enabled: bool,
    /// Automatically read assistant replies aloud
    #[serde(default = "default_true")]
    pub auto_read_replies: bool,
    /// "piper" | "system" | "audio8"
    #[serde(default = "default_engine")]
    pub engine: String,
    /// Piper voice id (e.g. it_IT-riccardo-x_low) or system voice hint
    #[serde(default = "default_voice")]
    pub voice: String,
    /// Playback rate 0.5–2.0
    #[serde(default = "default_rate")]
    pub rate: f32,
    /// Volume 0.1–2.0
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// Optional path to piper binary
    #[serde(default)]
    pub piper_bin: String,
    /// Directory with .onnx voice models (default ~/.agentos/voice/piper)
    #[serde(default)]
    pub piper_data_dir: String,
    /// Enable microphone → Whisper STT
    #[serde(default)]
    pub stt_enabled: bool,
    /// Optional path to whisper.cpp binary
    #[serde(default)]
    pub whisper_bin: String,
    /// Path to ggml model file
    #[serde(default)]
    pub whisper_model: String,
    /// Read aloud sentence-by-sentence while the LLM is still streaming (piper/audio8)
    #[serde(default = "default_true")]
    pub stream_sentences: bool,
    /// Audio8 local HTTP base (ONNX CPU service), e.g. http://127.0.0.1:8024
    #[serde(default = "default_audio8_url")]
    pub audio8_base_url: String,
    /// Where Audio8 runs: "cpu" (ONNX, recommended with Ollama on GPU) | "cuda"
    #[serde(default = "default_audio8_device")]
    pub audio8_device: String,
    /// Registered voice name in Audio8 service (ONNX) — leave empty to try default
    #[serde(default)]
    pub audio8_voice: String,
    /// HF model id for CUDA / transformers path
    #[serde(default = "default_audio8_model")]
    pub audio8_model_id: String,
    /// Optional path to Audio8_TTS/onnx_runtime checkout
    #[serde(default)]
    pub audio8_runtime_dir: String,
}

fn default_true() -> bool {
    true
}
fn default_engine() -> String {
    "system".into()
}
fn default_voice() -> String {
    "it_IT-riccardo-x_low".into()
}
fn default_rate() -> f32 {
    1.0
}
fn default_volume() -> f32 {
    1.0
}
fn default_audio8_url() -> String {
    "http://127.0.0.1:8024".into()
}
fn default_audio8_device() -> String {
    "cpu".into()
}
fn default_audio8_model() -> String {
    "Audio8/Audio8-TTS-Preview-0.6b".into()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_read_replies: true,
            engine: default_engine(),
            voice: default_voice(),
            rate: default_rate(),
            volume: default_volume(),
            piper_bin: String::new(),
            piper_data_dir: String::new(),
            stt_enabled: false,
            whisper_bin: String::new(),
            whisper_model: String::new(),
            stream_sentences: true,
            audio8_base_url: default_audio8_url(),
            audio8_device: default_audio8_device(),
            audio8_voice: String::new(),
            audio8_model_id: default_audio8_model(),
            audio8_runtime_dir: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefingConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Local hour when briefing is offered (0 = anytime)
    #[serde(default = "default_briefing_hour")]
    pub hour: u32,
    /// Ask LLM to polish the narrative
    #[serde(default)]
    pub llm_polish: bool,
}

fn default_briefing_hour() -> u32 {
    8
}

impl Default for BriefingConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            hour: default_briefing_hour(),
            llm_polish: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Inject skill bodies into the system prompt
    #[serde(default = "default_true")]
    pub inject_prompt: bool,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            inject_prompt: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub user_char_limit: usize,
    pub memory_char_limit: usize,
    pub write_approval: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionsConfig {
    pub default_mcp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpOAuthConfig {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub auth_url: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub extra_auth_params: HashMap<String, String>,
}

impl McpOAuthConfig {
    pub fn is_configured(&self) -> bool {
        !self.client_id.trim().is_empty()
            && !self.auth_url.trim().is_empty()
            && !self.token_url.trim().is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpWatchConfig {
    #[serde(default)]
    pub name: String,
    /// Exact MCP tool name. If empty, `tool_hint` is matched against discovered tools.
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub tool_hint: String,
    #[serde(default)]
    pub arguments: serde_json::Value,
    #[serde(default = "default_watch_interval")]
    pub interval_secs: u64,
    #[serde(default)]
    pub title: String,
    #[serde(default = "default_true")]
    pub notify_on_change: bool,
    /// If > 0, also notify when a datetime in the result is within this many minutes.
    #[serde(default)]
    pub upcoming_minutes: u64,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_watch_interval() -> u64 {
    300
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    #[serde(default = "default_mcp_transport")]
    pub transport: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Streamable HTTP / SSE endpoint (when transport is `http`).
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
    #[serde(default)]
    pub oauth: Option<McpOAuthConfig>,
    #[serde(default)]
    pub watches: Vec<McpWatchConfig>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_mcp_transport() -> String {
    "stdio".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: "ollama".into(),
            model: "qwen3.5:9b-q4km".into(),
            ollama: OllamaConfig {
                base_url: "http://127.0.0.1:11434".into(),
            },
            openai: CloudConfig {
                base_url: "https://api.openai.com/v1".into(),
            },
            anthropic: CloudConfig {
                base_url: "https://api.anthropic.com".into(),
            },
            memory: MemoryConfig {
                user_char_limit: 1375,
                memory_char_limit: 2200,
                write_approval: true,
            },
            mcp_servers: vec![],
            permissions: PermissionsConfig {
                default_mcp: "operate".into(),
            },
            avatar: default_avatar(),
            avatar_roam: false,
            voice: VoiceConfig::default(),
            briefing: BriefingConfig::default(),
            skills: SkillsConfig::default(),
            ui: UiConfig::default(),
        }
    }
}

pub fn data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agentos")
}

pub fn ensure_data_dirs() -> Result<PathBuf, String> {
    let root = data_dir();
    for sub in [
        "memory",
        "logs",
        "avatar",
        "voice",
        "voice/piper",
        "voice/whisper",
        "voice/tmp",
        "skills",
    ] {
        fs::create_dir_all(root.join(sub)).map_err(|e| e.to_string())?;
    }
    Ok(root)
}

pub fn config_path() -> PathBuf {
    data_dir().join("config.yaml")
}

pub fn load_config() -> Result<AppConfig, String> {
    let root = ensure_data_dirs()?;
    let path = root.join("config.yaml");
    if !path.exists() {
        let cfg = AppConfig::default();
        save_config(&cfg)?;
        return Ok(cfg);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_yaml::from_str(&raw).map_err(|e| e.to_string())
}

pub fn save_config(cfg: &AppConfig) -> Result<(), String> {
    ensure_data_dirs()?;
    let raw = serde_yaml::to_string(cfg).map_err(|e| e.to_string())?;
    fs::write(config_path(), raw).map_err(|e| e.to_string())
}

pub fn provider_badge(provider: &str) -> String {
    match provider {
        "ollama" => "Locale · Ollama".into(),
        "openai" => "Cloud · OpenAI".into(),
        "anthropic" => "Cloud · Anthropic".into(),
        other => format!("Provider · {other}"),
    }
}

pub fn session_badge(provider: &str, web: bool) -> String {
    let base = provider_badge(provider);
    if web {
        format!("{base} · Web")
    } else {
        base
    }
}
