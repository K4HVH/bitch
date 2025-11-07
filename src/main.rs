mod config;
mod proxy;
mod rules;

use anyhow::Result;
use tracing_subscriber::filter::LevelFilter;

use crate::config::Config;
use crate::proxy::ProxyServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Load and validate configuration
    let config = Config::load("config.toml")?;
    config.validate()?;

    // Initialize logging
    init_logging(&config.logging.level);

    // Create and run the proxy server
    let server = ProxyServer::new(config);
    server.run().await
}

fn init_logging(level: &str) {
    let filter = match level.to_lowercase().as_str() {
        "trace" => LevelFilter::TRACE,
        "debug" => LevelFilter::DEBUG,
        "info" => LevelFilter::INFO,
        "warn" => LevelFilter::WARN,
        "error" => LevelFilter::ERROR,
        _ => LevelFilter::INFO,
    };

    tracing_subscriber::fmt()
        .with_max_level(filter)
        .with_target(false)
        .init();
}
