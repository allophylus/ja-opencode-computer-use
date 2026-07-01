use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserConfig {
    pub enabled: bool,
    pub debug_url: String,
    pub auto_launch: bool,
    pub browser_path: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub transport: TransportConfig,
    pub display: DisplayConfig,
    pub sandbox: SandboxConfig,
    pub vision: VisionConfig,
    pub llm: LlmRawConfig,
    pub browser: BrowserConfig,
    pub logging: LoggingConfig,
    pub confirm: ConfirmConfig,
    pub audit: AuditConfig,
    pub rate_limit: RateLimitConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmConfig {
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditConfig {
    pub enabled: bool,
    pub max_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    pub enabled: bool,
    pub max_actions: u32,
    pub window_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LlmProviderType {
    Ollama,
    OpenAI,
    Anthropic,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmRawConfig {
    pub enabled: bool,
    pub provider: LlmProviderType,
    pub text_model: String,
    pub vision_model: String,
    pub endpoint: String,
    pub api_key: Option<String>,
    pub model_path: Option<String>,
    pub n_gpu_layers: u32,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportConfig {
    Stdio,
    Sse { port: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub number: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub enabled: bool,
    pub allowed_paths: Vec<String>,
    pub allowed_network: Vec<String>,
    pub allowed_commands: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionConfig {
    pub enabled: bool,
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub file: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            transport: TransportConfig::Stdio,
            display: DisplayConfig { number: 0 },
            sandbox: SandboxConfig {
                enabled: true,
                allowed_paths: vec![],
                allowed_network: vec![],
                allowed_commands: vec![],
            },
            vision: VisionConfig {
                enabled: false,
                provider: "ollama".into(),
                model: "llama3.2-vision".into(),
                endpoint: "http://localhost:11434".into(),
                api_key: None,
            },
            llm: LlmRawConfig {
                enabled: false,
                provider: LlmProviderType::Ollama,
                text_model: "llama3.2:3b".into(),
                vision_model: "llama3.2-vision:11b".into(),
                endpoint: "http://localhost:11434".into(),
                api_key: None,
                model_path: None,
                n_gpu_layers: 0,
                max_tokens: 2048,
                temperature: 0.1,
            },
            browser: BrowserConfig {
                enabled: false,
                debug_url: "http://localhost:9222".into(),
                auto_launch: false,
                browser_path: None,
            },
            logging: LoggingConfig {
                level: "info".into(),
                file: None,
            },
            confirm: ConfirmConfig {
                enabled: true,
            },
            audit: AuditConfig {
                enabled: true,
                max_entries: 1000,
            },
            rate_limit: RateLimitConfig {
                enabled: true,
                max_actions: 30,
                window_secs: 1,
            },
        }
    }
}

impl Config {
    pub async fn load(path: Option<&str>) -> Result<Self> {
        if let Some(path) = path {
            let content = tokio::fs::read_to_string(path).await?;
            Ok(serde_json::from_str(&content)?)
        } else {
            Ok(Self::default())
        }
    }
}
