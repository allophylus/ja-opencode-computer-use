use anyhow::Result;
use clap::Parser;
use opencode_computer_use::{config::Config, server::run_server};
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "ocu", about = "Desktop control MCP server for AI agents")]
struct Cli {
    /// MCP transport: stdio or sse
    #[arg(long, default_value = "stdio")]
    transport: String,

    /// Port for SSE transport
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Display number to capture (0 = main display)
    #[arg(long, default_value_t = 0)]
    display: u32,

    /// Enable vision fallback via screenshot + VLM
    #[arg(long)]
    vision: bool,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,

    /// Path to config file
    #[arg(long)]
    config: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&cli.log_level))
        .with_writer(std::io::stderr)
        .init();

    let config = Config::load(cli.config.as_deref()).await?;
    tracing::info!("Starting ocu server (transport={})", cli.transport);

    run_server(&config).await?;

    Ok(())
}
