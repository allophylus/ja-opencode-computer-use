use anyhow::Result;
use base64::Engine;
use super::{LlmConfig, LlmResult};

pub struct OllamaBackend {
    client: reqwest::Client,
    endpoint: String,
}

impl OllamaBackend {
    pub fn new(endpoint: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
        }
    }

    pub async fn generate(&self, model: &str, prompt: &str, _config: &LlmConfig) -> Result<LlmResult> {
        let body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "stream": false,
            "options": {
                "num_predict": _config.max_tokens,
                "temperature": _config.temperature,
            },
        });

        let resp = self
            .client
            .post(format!("{}/api/generate", self.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        let text = result["response"].as_str().unwrap_or("").to_string();
        let tokens = result["eval_count"].as_u64().unwrap_or(0) as u32;

        Ok(LlmResult {
            text,
            tokens_used: tokens,
            provider: "ollama".into(),
        })
    }

    pub async fn analyze_image(&self, model: &str, prompt: &str, image_png: &[u8], _config: &LlmConfig) -> Result<LlmResult> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_png);

        let body = serde_json::json!({
            "model": model,
            "prompt": prompt,
            "images": [b64],
            "stream": false,
            "options": {
                "num_predict": _config.max_tokens,
                "temperature": _config.temperature,
            },
        });

        let resp = self
            .client
            .post(format!("{}/api/generate", self.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        let text = result["response"].as_str().unwrap_or("").to_string();
        let tokens = result["eval_count"].as_u64().unwrap_or(0) as u32;

        Ok(LlmResult {
            text,
            tokens_used: tokens,
            provider: "ollama".into(),
        })
    }
}
