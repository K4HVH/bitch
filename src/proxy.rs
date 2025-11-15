use crate::batch::{BatchManager, BatchResult, Destination};
use crate::config::Config;
use crate::modifiers::ModifierManager;
use crate::plugins::PluginManager;
use crate::rules::{parse_mavlink_message, Action, AckInfo, ProcessResult, RuleEngine};
use anyhow::{Context, Result};
use mavlink::ardupilotmega::MavMessage;
use mavlink::{MavHeader, MavlinkVersion};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Shared state for the proxy
pub struct ProxyState {
    batch_manager: BatchManager,
}

impl ProxyState {
    pub fn new() -> Self {
        Self {
            batch_manager: BatchManager::new(),
        }
    }
}

/// Read a single MAVLink packet from an async reader
async fn read_mavlink_packet<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    // MAVLink v2 magic byte
    const MAVLINK_V2_MAGIC: u8 = 0xFD;

    // Read until we find a magic byte
    let magic = loop {
        let mut byte = [0u8; 1];
        reader.read_exact(&mut byte).await.context("Failed to read magic byte")?;
        if byte[0] == MAVLINK_V2_MAGIC {
            break byte[0];
        }
    };

    // Read payload length and incompatibility flags
    let mut header_buf = [0u8; 2];
    reader.read_exact(&mut header_buf).await.context("Failed to read header")?;
    let payload_len = header_buf[0] as usize;

    // Read rest of header (7 more bytes after magic, len, incompat)
    let mut rest_header = [0u8; 7];
    reader.read_exact(&mut rest_header).await.context("Failed to read rest of header")?;

    // Read payload
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload).await.context("Failed to read payload")?;

    // Read checksum (2 bytes)
    let mut checksum = [0u8; 2];
    reader.read_exact(&mut checksum).await.context("Failed to read checksum")?;

    // Reconstruct complete packet
    let mut packet = Vec::with_capacity(10 + payload_len + 2);
    packet.push(magic);
    packet.extend_from_slice(&header_buf);
    packet.extend_from_slice(&rest_header);
    packet.extend_from_slice(&payload);
    packet.extend_from_slice(&checksum);

    Ok(packet)
}

/// Main proxy server that handles bidirectional TCP forwarding
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
        // Initialize rule state manager with default states from config
        let initial_states: std::collections::HashMap<String, bool> = config
            .rules
            .iter()
            .map(|rule| (rule.name.clone(), rule.enabled_by_default))
            .collect();

        let state_manager = Arc::new(crate::rule_state::RuleStateManager::new(initial_states));

        // Spawn background task to clean up expired rule activations
        state_manager.clone().spawn_cleanup_task();

        let rule_engine = RuleEngine::new(
            config.rules.clone(),
            plugin_manager,
            modifier_manager,
            state_manager,
        )?;

        Ok(Self {
            config: Arc::new(config),
            rule_engine: Arc::new(rule_engine),
            state: Arc::new(ProxyState::new()),
        })
    }
}

/// Execute a sequence of actions on multiple packets
/// Called from message handlers and batch timeout handlers
pub fn execute_actions_impl(
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
                    Destination::Router(writer) => {
                        let mut stream = writer.write().await;
                        if let Err(e) = stream.write_all(&packet).await {
                            error!("Failed to forward packet to router: {}", e);
                        }
                    }
                    Destination::Gcs(writer) => {
                        let mut stream = writer.write().await;
                        if let Err(e) = stream.write_all(&packet).await {
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
                execute_actions_impl(remaining_actions, packets, destination, state).await;
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
                        Destination::Gcs(_) => "Router->GCS",
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
                    execute_actions_impl(
                        remaining_actions,
                        modified_packets,
                        destination,
                        state,
                    )
                    .await;
                } else {
                    warn!("Modify action has no modified message, forwarding original");
                    execute_actions_impl(remaining_actions, packets, destination, state)
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
                    execute_actions_impl(remaining_actions, packets, destination, state)
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
                        ProxyServer::extract_system_id_from_message(&msg, field_name).unwrap_or(header.system_id)
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
                        state.clone(),
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
                        execute_actions_impl(
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

impl ProxyServer {
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

        // Bind TCP listener for GCS connections
        let gcs_listener = TcpListener::bind(format!(
            "{}:{}",
            self.config.network.gcs_listen_address, self.config.network.gcs_listen_port
        ))
        .await
        .context("Failed to bind GCS TCP listener")?;

        info!("TCP listener initialized");

        // Accept GCS connection
        info!("Waiting for GCS connection...");
        let (gcs_stream, gcs_addr) = gcs_listener.accept().await.context("Failed to accept GCS connection")?;
        info!("GCS connected from: {}", gcs_addr);

        // Connect to mavlink-router
        let router_addr = format!(
            "{}:{}",
            self.config.network.router_address, self.config.network.router_port
        );
        let router_stream = TcpStream::connect(&router_addr)
            .await
            .context("Failed to connect to mavlink-router")?;
        info!("Connected to mavlink-router at {}", router_addr);

        // Split streams for concurrent reading/writing
        let (gcs_read, gcs_write) = gcs_stream.into_split();
        let (router_read, router_write) = router_stream.into_split();

        let gcs_write = Arc::new(RwLock::new(gcs_write));
        let router_write = Arc::new(RwLock::new(router_write));

        // Spawn GCS -> Router forwarding task
        let gcs_to_router_task = {
            let router_write = router_write.clone();
            let gcs_write = gcs_write.clone();
            let state = self.state.clone();
            let rule_engine = self.rule_engine.clone();

            tokio::spawn(async move {
                Self::forward_gcs_to_router(gcs_read, router_write, gcs_write, state, rule_engine).await
            })
        };

        // Spawn Router -> GCS forwarding task
        let router_to_gcs_task = {
            let gcs_write = gcs_write.clone();
            let router_write = router_write.clone();
            let state = self.state.clone();
            let rule_engine = self.rule_engine.clone();

            tokio::spawn(async move {
                Self::forward_router_to_gcs(router_read, gcs_write, router_write, state, rule_engine).await
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
        mut gcs_read: tokio::net::tcp::OwnedReadHalf,
        router_write: Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>,
        gcs_write: Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>,
        state: Arc<ProxyState>,
        rule_engine: Arc<RuleEngine>,
    ) -> Result<()> {
        info!("GCS->Router forwarding started");

        loop {
            // Read MAVLink packet from GCS
            let packet = match read_mavlink_packet(&mut gcs_read).await {
                Ok(pkt) => pkt,
                Err(e) => {
                    error!("Error reading from GCS: {}", e);
                    break;
                }
            };

            debug!("GCS->Router: {} bytes", packet.len());

            // Try to parse and process the MAVLink message
            let result = if let Ok((header, msg)) = parse_mavlink_message(&packet) {
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
                        let mut writer = gcs_write.write().await;
                        if let Err(e) = writer.write_all(&ack_packet).await {
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
            execute_actions_impl(
                result.actions,
                vec![packet],
                Destination::Router(router_write.clone()),
                state.clone(),
            )
            .await;
        }

        Ok(())
    }

    /// Forward messages from Router to GCS
    async fn forward_router_to_gcs(
        mut router_read: tokio::net::tcp::OwnedReadHalf,
        gcs_write: Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>,
        router_write: Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>,
        state: Arc<ProxyState>,
        rule_engine: Arc<RuleEngine>,
    ) -> Result<()> {
        info!("Router->GCS forwarding started");

        loop {
            // Read MAVLink packet from Router
            let packet = match read_mavlink_packet(&mut router_read).await {
                Ok(pkt) => pkt,
                Err(e) => {
                    error!("Error reading from router: {}", e);
                    break;
                }
            };

            debug!("Router->GCS: {} bytes", packet.len());

            // Try to parse and process the MAVLink message
            let result = if let Ok((header, msg)) = parse_mavlink_message(&packet) {
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
                        let mut writer = router_write.write().await;
                        if let Err(e) = writer.write_all(&ack_packet).await {
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
            execute_actions_impl(
                result.actions,
                vec![packet],
                Destination::Gcs(gcs_write.clone()),
                state.clone(),
            )
            .await;
        }

        Ok(())
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
