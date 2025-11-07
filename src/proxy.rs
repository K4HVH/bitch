use crate::config::Config;
use crate::rules::{parse_mavlink_message, ProcessResult, RuleEngine};
use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Shared state for the proxy
pub struct ProxyState {
    gcs_addr: RwLock<Option<SocketAddr>>,
}

impl ProxyState {
    pub fn new() -> Self {
        Self {
            gcs_addr: RwLock::new(None),
        }
    }
}

/// Main proxy server that handles bidirectional UDP forwarding
pub struct ProxyServer {
    config: Arc<Config>,
    rule_engine: Arc<RuleEngine>,
    state: Arc<ProxyState>,
}

impl ProxyServer {
    pub fn new(config: Config) -> Self {
        let rule_engine = RuleEngine::new(config.rules.clone());

        Self {
            config: Arc::new(config),
            rule_engine: Arc::new(rule_engine),
            state: Arc::new(ProxyState::new()),
        }
    }

    /// Start the proxy server
    pub async fn run(&self) -> Result<()> {
        info!("BITCH MAVLINK Interceptor starting...");
        info!(
            "   GCS listening on {}:{}",
            self.config.network.gcs_listen_address, self.config.network.gcs_listen_port
        );
        info!(
            "   Router at {}:{}",
            self.config.network.router_address, self.config.network.router_port
        );
        info!("   Rules loaded: {}", self.config.rules.len());

        for rule in &self.config.rules {
            info!(
                "   - {} {} -> {} {}",
                rule.message_type,
                rule.command
                    .as_ref()
                    .map(|cmd| format!("({})", cmd))
                    .unwrap_or_default(),
                rule.action,
                rule.delay_seconds
                    .map(|d| format!("({}s)", d))
                    .unwrap_or_default()
            );
        }

        // Bind GCS listener socket
        let gcs_socket = Arc::new(
            UdpSocket::bind(format!(
                "{}:{}",
                self.config.network.gcs_listen_address, self.config.network.gcs_listen_port
            ))
            .await
            .context("Failed to bind GCS listener socket")?,
        );

        // Create router socket
        let router_socket = Arc::new(
            UdpSocket::bind("0.0.0.0:0")
                .await
                .context("Failed to create router socket")?,
        );

        let router_addr = format!(
            "{}:{}",
            self.config.network.router_address, self.config.network.router_port
        );
        router_socket
            .connect(&router_addr)
            .await
            .context("Failed to connect to mavlink-router")?;

        info!("Sockets initialized");

        // Spawn GCS -> Router forwarding task
        let gcs_to_router_task = {
            let gcs_socket = gcs_socket.clone();
            let router_socket = router_socket.clone();
            let state = self.state.clone();
            let rule_engine = self.rule_engine.clone();

            tokio::spawn(async move {
                Self::forward_gcs_to_router(gcs_socket, router_socket, state, rule_engine).await
            })
        };

        // Spawn Router -> GCS forwarding task
        let router_to_gcs_task = {
            let gcs_socket = gcs_socket.clone();
            let router_socket = router_socket.clone();
            let state = self.state.clone();

            tokio::spawn(async move {
                Self::forward_router_to_gcs(router_socket, gcs_socket, state).await
            })
        };

        // Wait for both tasks
        tokio::select! {
            result = gcs_to_router_task => {
                error!("GCS->Router task ended: {:?}", result);
            }
            result = router_to_gcs_task => {
                error!("Router->GCS task ended: {:?}", result);
            }
        }

        Ok(())
    }

    /// Forward messages from GCS to Router with rule processing
    async fn forward_gcs_to_router(
        gcs_socket: Arc<UdpSocket>,
        router_socket: Arc<UdpSocket>,
        state: Arc<ProxyState>,
        rule_engine: Arc<RuleEngine>,
    ) -> Result<()> {
        let mut buf = vec![0u8; 65535];

        info!("GCS->Router forwarding started");

        loop {
            match gcs_socket.recv_from(&mut buf).await {
                Ok((len, addr)) => {
                    // Update GCS address if changed
                    {
                        let mut gcs_addr = state.gcs_addr.write().await;
                        if *gcs_addr != Some(addr) {
                            info!("GCS connected from: {}", addr);
                            *gcs_addr = Some(addr);
                        }
                    }

                    let packet = &buf[..len];
                    debug!("GCS->Router: {} bytes from {}", len, addr);

                    // Try to parse and process the MAVLink message
                    let result = if let Ok((header, msg)) = parse_mavlink_message(packet) {
                        rule_engine.process_message(&header, &msg)
                    } else {
                        // If we can't parse it, forward it anyway
                        debug!("Failed to parse message, forwarding anyway");
                        ProcessResult::Forward
                    };

                    match result {
                        ProcessResult::Forward => {
                            // Forward immediately
                            if let Err(e) = router_socket.send(packet).await {
                                error!("Failed to forward to router: {}", e);
                            } else {
                                debug!("Forwarded immediately");
                            }
                        }
                        ProcessResult::Delay(duration) => {
                            // Spawn a task to delay and forward
                            let router_socket = router_socket.clone();
                            let packet = packet.to_vec();
                            let delay_secs = duration.as_secs();

                            info!(
                                "Message queued for {}s delay (other traffic continues)",
                                delay_secs
                            );

                            tokio::spawn(async move {
                                sleep(duration).await;
                                if let Err(e) = router_socket.send(&packet).await {
                                    error!("Failed to send delayed packet: {}", e);
                                } else {
                                    info!("Delayed message forwarded after {}s", delay_secs);
                                }
                            });

                            // Loop continues immediately - other traffic flows normally
                        }
                        ProcessResult::Block => {
                            warn!("Message blocked by rule");
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving from GCS: {}", e);
                }
            }
        }
    }

    /// Forward messages from Router to GCS (transparent)
    async fn forward_router_to_gcs(
        router_socket: Arc<UdpSocket>,
        gcs_socket: Arc<UdpSocket>,
        state: Arc<ProxyState>,
    ) -> Result<()> {
        let mut buf = vec![0u8; 65535];

        info!("Router->GCS forwarding started");

        loop {
            match router_socket.recv(&mut buf).await {
                Ok(len) => {
                    debug!("Router->GCS: {} bytes", len);

                    // Get current GCS address
                    let gcs_addr = state.gcs_addr.read().await;

                    if let Some(addr) = *gcs_addr {
                        if let Err(e) = gcs_socket.send_to(&buf[..len], addr).await {
                            error!("Failed to forward to GCS: {}", e);
                        }
                    } else {
                        debug!("No GCS connected, dropping packet");
                    }
                }
                Err(e) => {
                    error!("Error receiving from router: {}", e);
                }
            }
        }
    }
}
