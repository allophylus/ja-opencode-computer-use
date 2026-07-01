use anyhow::Result;
use serde_json::Value;

/// Configuration for the vision fallback provider
#[derive(Debug, Clone)]
pub struct VisionConfig {
    pub provider: String,
    pub model: String,
    pub endpoint: String,
    pub api_key: Option<String>,
}

/// Result from a vision model: element labels with bounding boxes
#[derive(Debug, Clone, serde::Serialize)]
pub struct VisionResult {
    pub elements: Vec<VisionElement>,
    pub summary: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct VisionElement {
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Vision analyser that uses an external VLM to interpret screenshots.
pub struct VisionAnalyser {
    config: VisionConfig,
    client: reqwest::Client,
}

impl VisionAnalyser {
    pub fn new(config: VisionConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
        }
    }

    /// Analyse a screenshot PNG and return detected UI elements.
    /// The VLM is prompted to return structured JSON with bounding boxes.
    pub async fn analyse(&self, png_bytes: &[u8], prompt: Option<&str>) -> Result<VisionResult> {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(png_bytes);

        let default_prompt = "List all interactive UI elements visible in this screenshot. Return a JSON object with an \"elements\" array containing objects with: label (short description), x, y, width, height (in pixels, relative to the image dimensions). Also include a \"summary\" string describing the overall layout.";
        let prompt = prompt.unwrap_or(default_prompt);

        match self.config.provider.as_str() {
            "ollama" => self.analyse_ollama(&b64, prompt).await,
            "openai" => self.analyse_openai(&b64, prompt).await,
            "anthropic" => self.analyse_anthropic(&b64, prompt).await,
            provider => Err(anyhow::anyhow!("Unknown vision provider: {}", provider)),
        }
    }

    fn parse_vlm_response(&self, text: &str) -> VisionResult {
        // Try to extract JSON from the response (it may be wrapped in markdown code blocks)
        let body = text
            .trim()
            .strip_prefix("```json")
            .or_else(|| text.strip_prefix("```"))
            .and_then(|s| s.strip_suffix("```"))
            .map(|s| s.trim())
            .unwrap_or(text);

        // Try parsing as JSON
        if let Ok(parsed) = serde_json::from_str::<Value>(body) {
            let elements = parsed["elements"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|e| {
                            Some(VisionElement {
                                label: e["label"].as_str()?.to_string(),
                                x: e["x"].as_i64()? as i32,
                                y: e["y"].as_i64()? as i32,
                                width: e["width"].as_u64()? as u32,
                                height: e["height"].as_u64()? as u32,
                            })
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let summary = parsed["summary"]
                .as_str()
                .unwrap_or(text)
                .to_string();
            return VisionResult { elements, summary };
        }

        VisionResult {
            elements: vec![],
            summary: text.to_string(),
        }
    }

    async fn analyse_ollama(&self, b64: &str, prompt: &str) -> Result<VisionResult> {
        let body = serde_json::json!({
            "model": self.config.model,
            "prompt": prompt,
            "images": [b64],
            "stream": false,
        });

        let resp = self
            .client
            .post(format!("{}/api/generate", self.config.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: Value = resp.json().await?;
        let text = result["response"].as_str().unwrap_or("");
        Ok(self.parse_vlm_response(text))
    }

    async fn analyse_openai(&self, b64: &str, prompt: &str) -> Result<VisionResult> {
        let body = serde_json::json!({
            "model": self.config.model,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    {
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:image/png;base64,{}", b64),
                            "detail": "high"
                        }
                    }
                ]
            }]
        });

        let resp = self
            .client
            .post(format!("{}/v1/chat/completions", self.config.endpoint))
            .json(&body)
            .send()
            .await?;

        let result: Value = resp.json().await?;
        let text = result["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("");
        Ok(self.parse_vlm_response(text))
    }

    async fn analyse_anthropic(&self, b64: &str, prompt: &str) -> Result<VisionResult> {
        // Determine API key from config or env
        let api_key = self.config.api_key.as_deref()
            .and_then(|k| k.strip_prefix('$').and_then(|env_var| std::env::var(env_var).ok()))
            .or_else(|| self.config.api_key.as_deref().map(|s| s.to_string()))
            .unwrap_or_default();

        // Determine endpoint — default to Anthropic API
        let endpoint = if self.config.endpoint.contains("api.anthropic.com") || self.config.endpoint.contains("localhost") {
            self.config.endpoint.trim_end_matches('/').to_string()
        } else {
            "https://api.anthropic.com".to_string()
        };

        let body = serde_json::json!({
            "model": self.config.model,
            "max_tokens": 4096,
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "text", "text": prompt },
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": "image/png",
                            "data": b64
                        }
                    }
                ]
            }]
        });

        let resp = self
            .client
            .post(format!("{}/v1/messages", endpoint))
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;

        let result: Value = resp.json().await?;
        let text = result["content"][0]["text"]
            .as_str()
            .unwrap_or("");
        Ok(self.parse_vlm_response(text))
    }
}
