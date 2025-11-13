use crate::batch::{BatchManager, BatchResult, Destination};
use crate::config::Config;
use crate::modifiers::ModifierManager;
use crate::plugins::PluginManager;
use crate::rules::{parse_mavlink_message, Action, AckInfo, ProcessResult, RuleEngine};
use anyhow::{Context, Result};
use mavlink::ardupilotmega::MavMessage;
use mavlink::{MavHeader, MavlinkVersion};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Shared state for the proxy
pub struct ProxyState {
    gcs_addr: RwLock<Option<SocketAddr>>,
    batch_manager: BatchManager,
}

impl ProxyState {
    pub fn new() -> Self {
        Self {
            gcs_addr: RwLock::new(None),
            batch_manager: BatchManager::new(),
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
    pub fn new(
        config: Config,
        plugin_manager: PluginManager,
        modifier_manager: ModifierManager,
    ) -> Result<Self> {
        let rule_engine = RuleEngine::new(config.rules.clone(), plugin_manager, modifier_manager)?;

        Ok(Self {
            config: Arc::new(config),
            rule_engine: Arc::new(rule_engine),
            state: Arc::new(ProxyState::new()),
        })
    }

    /// Execute a sequence of actions on a packet
    async fn execute_actions(
        actions: Vec<Action>,
        packet: Vec<u8>,
        destination: Destination,
        state: Arc<ProxyState>,
    ) {
        Self::execute_actions_impl(actions, vec![packet], destination, state).await;
    }

    /// Execute a sequence of actions on multiple packets
    fn execute_actions_impl(
        mut actions: Vec<Action>,
        packets: Vec<Vec<u8>>,
        destination: Destination,
        state: Arc<ProxyState>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
        if actions.is_empty() {
            // No actions, forward all packets
            for packet in packets {
                match &destination {
                    Destination::Router(socket) => {
                        if let Err(e) = socket.send(&packet).await {
                            error!("Failed to forward packet to router: {}", e);
                        }
                    }
                    Destination::Gcs(socket, addr) => {
                        if let Err(e) = socket.send_to(&packet, addr).await {
                            error!("Failed to forward packet to GCS: {}", e);
                        }
                    }
                }
            }
            return;
        }

        // Take first action and process
        let action = actions.remove(0);
        let remaining_actions = actions;

        match action {
            Action::Forward => {
                // Forward and continue with remaining actions
                Self::execute_actions_impl(remaining_actions, packets, destination, state).await;
            }
            Action::Block => {
                warn!("Message(s) blocked by rule");
                // Don't process remaining actions
            }
            Action::Modify {
                modifier,
                modified_message,
            } => {
                // Modify action: replace message content with modified version
                if let Some(modified_msg) = modified_message {
                    let direction_label = match &destination {
                        Destination::Router(_) => "GCS->Router",
                        Destination::Gcs(_, _) => "Router->GCS",
                    };
                    info!("Applying modification from '{}' ({})", modifier, direction_label);

                    // Reconstruct packet with modified message
                    let mut modified_packets = Vec::new();

                    for packet in packets {
                        // Parse original packet to get header
                        if let Ok((header, _original_msg)) = parse_mavlink_message(&packet) {
                            // Serialize modified message
                            let mut buf = Vec::new();
                            if let Err(e) = mavlink::write_versioned_msg(
                                &mut buf,
                                MavlinkVersion::V2,
                                header,
                                &modified_msg,
                            ) {
                                error!("Failed to serialize modified message: {}", e);
                                modified_packets.push(packet); // Use original on error
                            } else {
                                modified_packets.push(buf);
                            }
                        } else {
                            warn!("Failed to parse packet for modification, using original");
                            modified_packets.push(packet);
                        }
                    }

                    // Continue with remaining actions using modified packets
                    Self::execute_actions_impl(
                        remaining_actions,
                        modified_packets,
                        destination,
                        state,
                    )
                    .await;
                } else {
                    warn!("Modify action has no modified message, forwarding original");
                    Self::execute_actions_impl(remaining_actions, packets, destination, state)
                        .await;
                }
            }
            Action::Delay(duration) => {
                // Spawn task to delay then continue with remaining actions
                let delay_secs = duration.as_secs();
                info!(
                    "Message(s) queued for {}s delay (other traffic continues)",
                    delay_secs
                );

                tokio::spawn(async move {
                    sleep(duration).await;
                    Self::execute_actions_impl(remaining_actions, packets, destination, state)
                        .await;
                    info!("Delayed message(s) forwarded after {}s", delay_secs);
                });
            }
            Action::Batch {
                count,
                timeout,
                key,
                forward_on_timeout,
                system_id_field,
            } => {
                // Batch action only makes sense for single packets
                if packets.len() != 1 {
                    warn!("Batch action applied to {} packets, only batching first", packets.len());
                }

                let packet = packets.into_iter().next().unwrap();

                // Extract system ID (generic for all message types)
                let system_id = if let Ok((header, msg)) = parse_mavlink_message(&packet) {
                    if let Some(ref field_name) = system_id_field {
                        // Extract from specified message field
                        Self::extract_system_id_from_message(&msg, field_name).unwrap_or(header.system_id)
                    } else {
                        // Default: use header system_id
                        header.system_id
                    }
                } else {
                    0
                };

                let batch_result = state
                    .batch_manager
                    .queue_or_release(
                        key.clone(),
                        system_id,
                        packet,
                        count,
                        timeout,
                        forward_on_timeout,
                        remaining_actions.clone(),
                        destination.clone(),
                    )
                    .await;

                match batch_result {
                    BatchResult::Queued => {
                        // Packet queued, nothing more to do
                    }
                    BatchResult::Release {
                        packets,
                        remaining_actions,
                    } => {
                        // Threshold met, apply remaining actions to all packets
                        info!(
                            "Batch '{}' threshold met, applying {} remaining action(s) to {} packets",
                            key,
                            remaining_actions.len(),
                            packets.len()
                        );
                        Self::execute_actions_impl(
                            remaining_actions,
                            packets,
                            destination,
                            state,
                        )
                        .await;
                    }
                }
            }
        }
        })
    }

    /// Extract system_id from a message field generically
    fn extract_system_id_from_message(msg: &MavMessage, field_name: &str) -> Option<u8> {
        // Serialize message to JSON (mavlink internally-tagged format)
        let message_json = serde_json::to_value(msg).ok()?;

        // Extract field value directly
        let field_value = message_json.get(field_name)?;
        field_value.as_u64().map(|v| v as u8)
    }

    /// Build a generic ACK message (works for ANY message type)
    fn build_ack(ack_info: &AckInfo) -> Result<Vec<u8>> {
        // Start with fields from config
        let mut fields_json = serde_json::Map::new();

        // Add the type field for internally-tagged enum
        fields_json.insert("type".to_string(), serde_json::Value::String(ack_info.message_type.clone()));

        // Copy fields from original message based on config
        for (ack_field, source_path) in &ack_info.copy_fields {
            let value = if source_path.starts_with("header.") {
                // Copy from header (e.g., "header.system_id")
                let header_field = source_path.trim_start_matches("header.");
                match header_field {
                    "system_id" => Some(serde_json::json!(ack_info.original_header.system_id)),
                    "component_id" => Some(serde_json::json!(ack_info.original_header.component_id)),
                    "sequence" => Some(serde_json::json!(ack_info.original_header.sequence)),
                    _ => {
                        warn!("Unknown header field: {}", header_field);
                        None
                    }
                }
            } else {
                // Copy from message payload (mavlink internally-tagged format)
                // Just copy the field as-is, preserving internally-tagged enum structure
                ack_info.original_message.get(source_path).cloned()
            };

            if let Some(val) = value {
                fields_json.insert(ack_field.clone(), val);
            } else {
                warn!("Failed to copy field '{}' from '{}'", ack_field, source_path);
            }
        }

        // Add configured fields (can override copied values)
        for (key, value) in &ack_info.fields {
            // Convert TOML value to JSON value directly (preserves structure)
            let json_value = toml_to_json(value);
            fields_json.insert(key.clone(), json_value);
        }

        // Deserialize to MavMessage using internally-tagged format
        let json_value = serde_json::Value::Object(fields_json);
        let msg: MavMessage = serde_json::from_value(json_value)
            .context("Failed to deserialize ACK message from fields")?;

        // Build header - ACK appears to come FROM the target system
        let header = MavHeader {
            system_id: ack_info.source_system,
            component_id: ack_info.source_component,
            sequence: 0, // TODO: track sequence numbers per system
        };

        // Serialize to bytes
        let mut buf = Vec::new();
        mavlink::write_versioned_msg(&mut buf, MavlinkVersion::V2, header, &msg)
            .context("Failed to serialize ACK message")?;

        Ok(buf)
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
            let actions_str = rule.get_actions().join(" -> ");
            info!(
                "   - {} -> {} {}",
                rule.message_type,
                actions_str,
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
            let rule_engine = self.rule_engine.clone();

            tokio::spawn(async move {
                Self::forward_router_to_gcs(router_socket, gcs_socket, state, rule_engine).await
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
                        rule_engine.process_message_with_direction(&header, &msg, "gcs_to_router")
                    } else {
                        // If we can't parse it, forward it anyway
                        debug!("Failed to parse message, forwarding anyway");
                        ProcessResult {
                            actions: vec![Action::Forward],
                            ack_info: None,
                        }
                    };

                    // Send ACK if auto_ack is enabled (works for ANY message type)
                    if let Some(ref ack_info) = result.ack_info {
                        match Self::build_ack(ack_info) {
                            Ok(ack_packet) => {
                                if let Err(e) = gcs_socket.send_to(&ack_packet, addr).await {
                                    error!("Failed to send {} to GCS: {}", ack_info.message_type, e);
                                } else {
                                    info!(
                                        "Sent {} to GCS (sysid={})",
                                        ack_info.message_type, ack_info.source_system
                                    );
                                }
                            }
                            Err(e) => {
                                error!("Failed to build {} message: {}", ack_info.message_type, e);
                            }
                        }
                    }

                    // Execute action sequence
                    Self::execute_actions(
                        result.actions,
                        packet.to_vec(),
                        Destination::Router(router_socket.clone()),
                        state.clone(),
                    )
                    .await;
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
        rule_engine: Arc<RuleEngine>,
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
                        let packet = &buf[..len];

                        // Try to parse and process the MAVLink message
                        let result = if let Ok((header, msg)) = parse_mavlink_message(packet) {
                            rule_engine.process_message_with_direction(&header, &msg, "router_to_gcs")
                        } else {
                            // If we can't parse it, forward it anyway
                            debug!("Failed to parse Router->GCS message, forwarding anyway");
                            ProcessResult {
                                actions: vec![Action::Forward],
                                ack_info: None,
                            }
                        };

                        // Send ACK if auto_ack is enabled (works for ANY message type)
                        if let Some(ref ack_info) = result.ack_info {
                            match Self::build_ack(ack_info) {
                                Ok(ack_packet) => {
                                    if let Err(e) = router_socket.send(&ack_packet).await {
                                        error!("Failed to send {} to router: {}", ack_info.message_type, e);
                                    } else {
                                        info!(
                                            "Sent {} to router (sysid={})",
                                            ack_info.message_type, ack_info.source_system
                                        );
                                    }
                                }
                                Err(e) => {
                                    error!("Failed to build {} message: {}", ack_info.message_type, e);
                                }
                            }
                        }

                        // Execute action sequence (router->GCS direction)
                        Self::execute_actions(
                            result.actions,
                            packet.to_vec(),
                            Destination::Gcs(gcs_socket.clone(), addr),
                            state.clone(),
                        )
                        .await;
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

/// Convert TOML value to JSON value, preserving structure
fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .unwrap_or(serde_json::Value::Null)
        }
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Array(arr) => {
            let json_arr: Vec<serde_json::Value> = arr.iter().map(toml_to_json).collect();
            serde_json::Value::Array(json_arr)
        }
        toml::Value::Table(table) => {
            let mut json_obj = serde_json::Map::new();
            for (k, v) in table {
                json_obj.insert(k.clone(), toml_to_json(v));
            }
            serde_json::Value::Object(json_obj)
        }
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
    }
}
