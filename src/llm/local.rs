//! Direct llama.cpp inference backend via `llama-cpp-2` crate.
//! Supports GGUF-quantized text and vision models.
//! Activated with `--features local-llm`.

use anyhow::{Context, Result};
use super::{LlmConfig, LlmResult};

pub struct LocalBackend {
    _model_path: String,
    _n_gpu_layers: u32,
}

impl LocalBackend {
    pub fn new(model_path: &str, n_gpu_layers: u32) -> Result<Self> {
        Ok(Self {
            _model_path: model_path.to_string(),
            _n_gpu_layers: n_gpu_layers,
        })
    }

    pub async fn generate(&self, _prompt: &str, _config: &LlmConfig) -> Result<LlmResult> {
        // TODO: real llama-cpp-2 text inference
        // let model = LlamaModel::load_from_file(&self._model_path, LlamaParams::default())?;
        // let mut ctx = model.create_session()?;
        // let tokens = ctx.advance_context(_prompt)?;
        // let text = ctx.decode(..)?;
        // Ok(LlmResult { text, tokens_used: tokens as u32, provider: "local".into() })

        Err(anyhow::anyhow!(
            "local llama.cpp backend requires `llama-cpp-2` crate integration. \
             Use provider = \"ollama\" for now (serves local models via HTTP)."
        ))
    }

    pub async fn analyze_image(&self, _prompt: &str, _image_png: &[u8], _config: &LlmConfig) -> Result<LlmResult> {
        Err(anyhow::anyhow!(
            "local vision inference requires multimodel GGUF (e.g. LLaVA, Llama 3.2 Vision). \
             Use provider = \"ollama\" with a vision model for now."
        ))
    }
}
