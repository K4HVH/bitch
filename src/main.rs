mod config;
mod plugins;
mod proxy;
mod rules;

use anyhow::Result;
use std::path::PathBuf;
use tracing::{info, warn};
use tracing_subscriber::filter::LevelFilter;

use crate::config::Config;
use crate::plugins::PluginManager;
use crate::proxy::ProxyServer;

#[tokio::main]
async fn main() -> Result<()> {
    // Load and validate configuration
    let config = Config::load("config.toml")?;
    config.validate()?;

    // Initialize logging
    init_logging(&config.logging.level);

    // Initialize plugin manager
    let mut plugin_manager = PluginManager::new()?;

    // Load plugins
    for (name, filename) in &config.plugins.load {
        let path = PathBuf::from(&config.plugins.directory).join(filename);
        match plugin_manager.load_plugin(name, &path) {
            Ok(_) => info!("Loaded plugin: {}", name),
            Err(e) => warn!("Failed to load plugin '{}': {}", name, e),
        }
    }

    // Create and run the proxy server
    let server = ProxyServer::new(config, plugin_manager)?;
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
