use anyhow::Result;
use base64::Engine;
use super::{LlmConfig, LlmResult};

pub struct AnthropicBackend {
    client: reqwest::Client,
    api_key: String,
    endpoint: String,
}

impl AnthropicBackend {
    pub fn new(api_key: &str, endpoint: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.to_string(),
            endpoint: endpoint.trim_end_matches('/').to_string(),
        }
    }

    fn headers(&self) -> reqwest::header::HeaderMap {
        let mut h = reqwest::header::HeaderMap::new();
        h.insert("x-api-key", self.api_key.parse().unwrap());
        h.insert("anthropic-version", "2023-06-01".parse().unwrap());
        h.insert("Content-Type", "application/json".parse().unwrap());
        h
    }

    pub async fn generate(&self, model: &str, prompt: &str, _config: &LlmConfig) -> Result<LlmResult> {
        let body = serde_json::json!({
            "model": model,
            "max_tokens": _config.max_tokens,
            "temperature": _config.temperature,
            "messages": [{"role": "user", "content": prompt}],
        });

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.endpoint))
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let tokens = result["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

        Ok(LlmResult { text, tokens_used: tokens, provider: "anthropic".into() })
    }

    pub async fn analyze_image(&self, model: &str, prompt: &str, image_png: &[u8], _config: &LlmConfig) -> Result<LlmResult> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(image_png);

        let body = serde_json::json!({
            "model": model,
            "max_tokens": _config.max_tokens,
            "temperature": _config.temperature,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": b64,
                        }
                    }
                ]
            }],
        });

        let resp = self
            .client
            .post(format!("{}/v1/messages", self.endpoint))
            .headers(self.headers())
            .json(&body)
            .send()
            .await?;

        let result: serde_json::Value = resp.json().await?;
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let tokens = result["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32;

        Ok(LlmResult { text, tokens_used: tokens, provider: "anthropic".into() })
    }
}
