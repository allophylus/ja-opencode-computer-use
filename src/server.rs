use anyhow::Result;
use tracing::info;

use crate::browser::CdpClient;
use crate::config::Config;
use crate::llm::LocalLlm;
use crate::mcp::{McpServer, ToolRegistry};
use crate::platform::Platform;
use crate::safety::{ConfirmationGate, RateLimiter, Sandbox};
use crate::transport::sse;
use crate::vision::{VisionAnalyser, VisionConfig};

/// Run the MCP server with the given configuration.
pub async fn run_server(config: &Config) -> Result<()> {
    let platform = Platform::new()?;

    let llm = LocalLlm::from_config(&config.llm)?;
    if let Some(ref _llm) = llm {
        let llm_config: crate::llm::LlmConfig = config.llm.clone().into();
        info!(
            "LLM enabled: {:?} (text: {}, vision: {})",
            llm_config.provider, llm_config.text_model, llm_config.vision_model
        );
    }

    let browser = if config.browser.enabled {
        match CdpClient::connect(&config.browser.debug_url).await {
            Ok(client) => {
                info!("Browser CDP connected: {}", config.browser.debug_url);
                Some(client)
            }
            Err(e) => {
                tracing::warn!("Browser CDP connection failed: {}. Browser tools will return errors.", e);
                None
            }
        }
    } else {
        None
    };

    let vision = if config.vision.enabled {
        info!("Vision fallback enabled: {} @ {}", config.vision.model, config.vision.endpoint);
        Some(VisionAnalyser::new(VisionConfig {
            provider: config.vision.provider.clone(),
            model: config.vision.model.clone(),
            endpoint: config.vision.endpoint.clone(),
            api_key: config.vision.api_key.clone(),
        }))
    } else {
        None
    };

    let sandbox = Sandbox::new(
        config.sandbox.enabled,
        config.sandbox.allowed_paths.clone(),
        config.sandbox.allowed_network.clone(),
        config.sandbox.allowed_commands.clone(),
    );

    let confirmation_gate = ConfirmationGate::new(config.confirm.enabled);
    let rate_limiter = RateLimiter::new(
        config.rate_limit.enabled,
        config.rate_limit.max_actions,
        config.rate_limit.window_secs,
    );

    if config.audit.enabled {
        info!("Audit log enabled (max {} entries)", config.audit.max_entries);
    }
    if config.confirm.enabled {
        info!("Confirmation gates enabled");
    }
    if config.rate_limit.enabled {
        info!("Rate limiting enabled: {} actions/{}s", config.rate_limit.max_actions, config.rate_limit.window_secs);
    }
    if config.sandbox.enabled {
        info!("Sandbox enabled: {} allowed paths, {} commands, {} network patterns",
            config.sandbox.allowed_paths.len(),
            config.sandbox.allowed_commands.len(),
            config.sandbox.allowed_network.len(),
        );
    }

    let tool_registry = ToolRegistry::new(platform, llm, browser, vision, sandbox, confirmation_gate, rate_limiter, "default".into());

    match &config.transport {
        crate::config::TransportConfig::Stdio => {
            info!("Starting MCP server on stdio (tools: {})", tool_registry.list().len());
            let server = McpServer::new(tool_registry);
            server.run().await?;
        }
        crate::config::TransportConfig::Sse { port } => {
            info!("Starting MCP server on SSE port {}", port);
            let server = McpServer::new(tool_registry);
            sse::run_sse(server, *port).await?;
        }
    }

    Ok(())
}
