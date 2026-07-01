//! LLM inference for vision analysis, action planning, and element matching.
//!
//! Providers:
//! - `ollama`   (default) — HTTP client to local Ollama server
//! - `openai`   — OpenAI / OpenAI-compatible API (GPT-4o, etc.)
//! - `anthropic`— Anthropic Messages API (Claude 3.5 Sonnet, etc.)
//! - `local`    (feature `local-llm`) — direct llama.cpp bindings

pub mod anthropic;
pub mod ollama;
pub mod openai;

#[cfg(feature = "local-llm")]
pub mod local;

use anyhow::Result;
use serde::Serialize;

/// Resolve an API key from config value or environment variable.
/// Supports: `$ENV_VAR_NAME` (env var reference) or literal key.
fn resolve_api_key(val: &Option<String>) -> Option<String> {
    let s = val.as_ref()?;
    if let Some(var) = s.strip_prefix('$') {
        std::env::var(var).ok()
    } else {
        Some(s.clone())
    }
}

// Convert from config types (avoids circular dependency by using crate::config path)
impl From<crate::config::LlmRawConfig> for LlmConfig {
    fn from(c: crate::config::LlmRawConfig) -> Self {
        let provider = match c.provider {
            crate::config::LlmProviderType::Ollama => {
                LlmProvider::Ollama { endpoint: c.endpoint.clone() }
            }
            crate::config::LlmProviderType::OpenAI => {
                LlmProvider::OpenAI {
                    api_key: resolve_api_key(&c.api_key).unwrap_or_default(),
                    endpoint: if c.endpoint.is_empty() {
                        "https://api.openai.com/v1".into()
                    } else {
                        c.endpoint.clone()
                    },
                }
            }
            crate::config::LlmProviderType::Anthropic => {
                LlmProvider::Anthropic {
                    api_key: resolve_api_key(&c.api_key).unwrap_or_default(),
                    endpoint: if c.endpoint.is_empty() {
                        "https://api.anthropic.com".into()
                    } else {
                        c.endpoint.clone()
                    },
                }
            }
            crate::config::LlmProviderType::Local => {
                #[cfg(feature = "local-llm")]
                {
                    LlmProvider::Local {
                        model_path: c.model_path.clone().unwrap_or_default(),
                        n_gpu_layers: c.n_gpu_layers,
                    }
                }
                #[cfg(not(feature = "local-llm"))]
                {
                    LlmProvider::Ollama { endpoint: c.endpoint.clone() }
                }
            }
        };
        LlmConfig {
            provider,
            text_model: c.text_model,
            vision_model: c.vision_model,
            max_tokens: c.max_tokens,
            temperature: c.temperature,
        }
    }
}

/// Unified inference result
#[derive(Debug, Clone, Serialize)]
pub struct LlmResult {
    pub text: String,
    pub tokens_used: u32,
    pub provider: String,
}

/// LLM configuration
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub text_model: String,
    pub vision_model: String,
    pub max_tokens: u32,
    pub temperature: f32,
}

#[derive(Debug, Clone)]
pub enum LlmProvider {
    Ollama { endpoint: String },
    OpenAI { api_key: String, endpoint: String },
    Anthropic { api_key: String, endpoint: String },
    #[cfg(feature = "local-llm")]
    Local { model_path: String, n_gpu_layers: u32 },
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::Ollama {
                endpoint: "http://localhost:11434".into(),
            },
            text_model: "llama3.2:3b".into(),
            vision_model: "llama3.2-vision:11b".into(),
            max_tokens: 2048,
            temperature: 0.1,
        }
    }
}

/// Unified LLM interface for text and vision inference
pub enum LocalLlm {
    Ollama(ollama::OllamaBackend),
    OpenAI(openai::OpenAiBackend),
    Anthropic(anthropic::AnthropicBackend),
    #[cfg(feature = "local-llm")]
    Local(local::LocalBackend),
}

impl LocalLlm {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        match &config.provider {
            LlmProvider::Ollama { endpoint } => {
                Ok(Self::Ollama(ollama::OllamaBackend::new(endpoint)))
            }
            LlmProvider::OpenAI { api_key, endpoint } => {
                Ok(Self::OpenAI(openai::OpenAiBackend::new(api_key, endpoint)))
            }
            LlmProvider::Anthropic { api_key, endpoint } => {
                Ok(Self::Anthropic(anthropic::AnthropicBackend::new(api_key, endpoint)))
            }
            #[cfg(feature = "local-llm")]
            LlmProvider::Local { model_path, n_gpu_layers } => {
                Ok(Self::Local(local::LocalBackend::new(model_path, *n_gpu_layers)?))
            }
        }
    }

    /// Build from config, with validation for feature-gated providers.
    pub fn from_config(cfg: &crate::config::LlmRawConfig) -> Result<Option<Self>> {
        if !cfg.enabled {
            return Ok(None);
        }
        // Validate local provider early (before From conversion)
        if matches!(cfg.provider, crate::config::LlmProviderType::Local) {
            #[cfg(not(feature = "local-llm"))]
            {
                anyhow::bail!(
                    "Provider 'local' requires building with --features local-llm. \
                     Use 'ollama' (default) for zero-dep local inference."
                );
            }
        }
        let config: LlmConfig = cfg.clone().into();
        Ok(Some(Self::new(&config)?))
    }

    /// Pure text inference (e.g. action planning, summarization)
    pub async fn generate(&self, model: &str, prompt: &str, config: &LlmConfig) -> Result<LlmResult> {
        match self {
            Self::Ollama(b) => b.generate(model, prompt, config).await,
            Self::OpenAI(b) => b.generate(model, prompt, config).await,
            Self::Anthropic(b) => b.generate(model, prompt, config).await,
            #[cfg(feature = "local-llm")]
            Self::Local(b) => b.generate(prompt, config).await,
        }
    }

    /// Vision inference (screenshot analysis)
    pub async fn analyze_image(&self, model: &str, prompt: &str, image_png: &[u8], config: &LlmConfig) -> Result<LlmResult> {
        match self {
            Self::Ollama(b) => b.analyze_image(model, prompt, image_png, config).await,
            Self::OpenAI(b) => b.analyze_image(model, prompt, image_png, config).await,
            Self::Anthropic(b) => b.analyze_image(model, prompt, image_png, config).await,
            #[cfg(feature = "local-llm")]
            Self::Local(b) => b.analyze_image(prompt, image_png, config).await,
        }
    }
}
