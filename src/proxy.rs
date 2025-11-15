use crate::batch::{BatchManager, BatchResult, Destination};
use crate::config::Config;
use crate::modifiers::ModifierManager;
use crate::plugins::PluginManager;
use crate::rules::{parse_mavlink_message, Action, AckInfo, ProcessResult, RuleEngine};
use anyhow::{Context, Result};
use mavlink::ardupilotmega::MavMessage;
use mavlink::{MavHeader, MavlinkVersion};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Unique identifier for each GCS client
type ClientId = u64;

/// Shared state for the proxy
pub struct ProxyState {
    batch_manager: BatchManager,
    /// Connected GCS clients (ClientId -> WriteHalf)
    gcs_clients: RwLock<HashMap<ClientId, Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>>>,
    /// Counter for generating unique client IDs
    next_client_id: AtomicU64,
}

impl ProxyState {
    pub fn new() -> Self {
        Self {
            batch_manager: BatchManager::new(),
            gcs_clients: RwLock::new(HashMap::new()),
            next_client_id: AtomicU64::new(1),
        }
    }

    /// Add a new GCS client and return its ID
    pub async fn add_gcs_client(&self, writer: tokio::net::tcp::OwnedWriteHalf) -> ClientId {
        let client_id = self.next_client_id.fetch_add(1, Ordering::SeqCst);
        let mut clients = self.gcs_clients.write().await;
        clients.insert(client_id, Arc::new(RwLock::new(writer)));
        info!("GCS client {} connected (total: {})", client_id, clients.len());
        client_id
    }

    /// Remove a GCS client
    pub async fn remove_gcs_client(&self, client_id: ClientId) {
        let mut clients = self.gcs_clients.write().await;
        clients.remove(&client_id);
        info!("GCS client {} disconnected (remaining: {})", client_id, clients.len());
    }

    /// Get a clone of a specific GCS client writer
    pub async fn get_gcs_client(&self, client_id: ClientId) -> Option<Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>> {
        let clients = self.gcs_clients.read().await;
        clients.get(&client_id).cloned()
    }

    /// Broadcast a packet to all connected GCS clients
    pub async fn broadcast_to_all_gcs(&self, packet: &[u8]) {
        let clients = self.gcs_clients.read().await;

        for (client_id, writer) in clients.iter() {
            let mut stream = writer.write().await;
            if let Err(e) = stream.write_all(packet).await {
                error!("Failed to send to GCS client {}: {}", client_id, e);
            }
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

/// Execute actions and broadcast result to all GCS clients
pub fn execute_actions_impl_broadcast(
    mut actions: Vec<Action>,
    packets: Vec<Vec<u8>>,
    state: Arc<ProxyState>,
) -> Pin<Box<dyn Future<Output = ()> + Send>> {
    Box::pin(async move {
        if actions.is_empty() {
            // No actions, broadcast all packets
            for packet in packets {
                state.broadcast_to_all_gcs(&packet).await;
            }
            return;
        }

        // Take first action and process
        let action = actions.remove(0);
        let remaining_actions = actions;

        match action {
            Action::Forward => {
                // Forward and continue with remaining actions
                execute_actions_impl_broadcast(remaining_actions, packets, state).await;
            }
            Action::Block => {
                warn!("Message(s) blocked by rule (broadcast direction)");
            }
            Action::Modify {
                modifier,
                modified_message,
            } => {
                if let Some(modified_msg) = modified_message {
                    info!("Applying modification from '{}' (Router->GCS broadcast)", modifier);

                    let mut modified_packets = Vec::new();
                    for packet in packets {
                        if let Ok((header, _original_msg)) = parse_mavlink_message(&packet) {
                            let mut buf = Vec::new();
                            if let Err(e) = mavlink::write_versioned_msg(
                                &mut buf,
                                MavlinkVersion::V2,
                                header,
                                &modified_msg,
                            ) {
                                error!("Failed to serialize modified message: {}", e);
                                modified_packets.push(packet);
                            } else {
                                modified_packets.push(buf);
                            }
                        } else {
                            warn!("Failed to parse packet for modification, using original");
                            modified_packets.push(packet);
                        }
                    }

                    execute_actions_impl_broadcast(remaining_actions, modified_packets, state).await;
                } else {
                    warn!("Modify action has no modified message, forwarding original");
                    execute_actions_impl_broadcast(remaining_actions, packets, state).await;
                }
            }
            Action::Delay(duration) => {
                let delay_secs = duration.as_secs();
                info!("Message(s) queued for {}s delay (broadcast)", delay_secs);

                tokio::spawn(async move {
                    sleep(duration).await;
                    execute_actions_impl_broadcast(remaining_actions, packets, state).await;
                    info!("Delayed broadcast message(s) forwarded after {}s", delay_secs);
                });
            }
            Action::Batch { .. } => {
                warn!("Batch action not supported in router->GCS direction, forwarding");
                execute_actions_impl_broadcast(remaining_actions, packets, state).await;
            }
        }
    })
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
                let Destination::Router(writer) = &destination;
                let mut stream = writer.write().await;
                if let Err(e) = stream.write_all(&packet).await {
                    error!("Failed to forward packet to router: {}", e);
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
                    info!("Applying modification from '{}' (GCS->Router)", modifier);

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

        // Connect to mavlink-router first (single persistent connection)
        let router_addr = format!(
            "{}:{}",
            self.config.network.router_address, self.config.network.router_port
        );
        let router_stream = TcpStream::connect(&router_addr)
            .await
            .context("Failed to connect to mavlink-router")?;
        info!("Connected to mavlink-router at {}", router_addr);

        // Split router stream
        let (router_read, router_write) = router_stream.into_split();
        let router_write = Arc::new(RwLock::new(router_write));

        // Bind TCP listener for GCS connections
        let gcs_listener = TcpListener::bind(format!(
            "{}:{}",
            self.config.network.gcs_listen_address, self.config.network.gcs_listen_port
        ))
        .await
        .context("Failed to bind GCS TCP listener")?;

        info!("TCP listener initialized, accepting multiple GCS connections...");

        // Spawn Router -> All GCS broadcast task
        let router_to_all_gcs_task = {
            let state = self.state.clone();
            let rule_engine = self.rule_engine.clone();
            let router_write = router_write.clone();

            tokio::spawn(async move {
                Self::forward_router_to_all_gcs(router_read, router_write, state, rule_engine).await
            })
        };

        // Accept GCS connections in a loop
        let gcs_accept_task = {
            let state = self.state.clone();
            let rule_engine = self.rule_engine.clone();
            let router_write = router_write.clone();

            tokio::spawn(async move {
                loop {
                    match gcs_listener.accept().await {
                        Ok((gcs_stream, gcs_addr)) => {
                            info!("New GCS connection from: {}", gcs_addr);

                            // Split the GCS stream
                            let (gcs_read, gcs_write) = gcs_stream.into_split();

                            // Register the client
                            let client_id = state.add_gcs_client(gcs_write).await;

                            // Spawn task to handle this GCS client (GCS -> Router)
                            let state_clone = state.clone();
                            let rule_engine_clone = rule_engine.clone();
                            let router_write_clone = router_write.clone();

                            tokio::spawn(async move {
                                if let Err(e) = Self::forward_gcs_to_router(
                                    client_id,
                                    gcs_read,
                                    router_write_clone,
                                    state_clone.clone(),
                                    rule_engine_clone,
                                )
                                .await
                                {
                                    error!("GCS client {} error: {}", client_id, e);
                                }

                                // Remove client on disconnect
                                state_clone.remove_gcs_client(client_id).await;
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept GCS connection: {}", e);
                        }
                    }
                }
            })
        };

        // Wait for tasks (router broadcast task should never end normally)
        tokio::select! {
            result = router_to_all_gcs_task => {
                error!("Router->GCS broadcast task ended: {:?}", result);
            }
            result = gcs_accept_task => {
                error!("GCS accept task ended: {:?}", result);
            }
        }

        Ok(())
    }

    /// Forward messages from a specific GCS client to Router with rule processing
    async fn forward_gcs_to_router(
        client_id: ClientId,
        mut gcs_read: tokio::net::tcp::OwnedReadHalf,
        router_write: Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>,
        state: Arc<ProxyState>,
        rule_engine: Arc<RuleEngine>,
    ) -> Result<()> {
        info!("GCS client {} -> Router forwarding started", client_id);

        loop {
            // Read MAVLink packet from this GCS client
            let packet = match read_mavlink_packet(&mut gcs_read).await {
                Ok(pkt) => pkt,
                Err(e) => {
                    debug!("GCS client {} read error: {}", client_id, e);
                    break;
                }
            };

            debug!("GCS client {} -> Router: {} bytes", client_id, packet.len());

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

            // Send ACK if auto_ack is enabled (to this specific GCS client)
            if let Some(ref ack_info) = result.ack_info {
                match Self::build_ack(ack_info) {
                    Ok(ack_packet) => {
                        if let Some(gcs_writer) = state.get_gcs_client(client_id).await {
                            let mut writer = gcs_writer.write().await;
                            if let Err(e) = writer.write_all(&ack_packet).await {
                                error!("Failed to send {} to GCS client {}: {}", ack_info.message_type, client_id, e);
                            } else {
                                info!(
                                    "Sent {} to GCS client {} (sysid={})",
                                    ack_info.message_type, client_id, ack_info.source_system
                                );
                            }
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

        info!("GCS client {} -> Router forwarding ended", client_id);
        Ok(())
    }

    /// Forward messages from Router to all connected GCS clients (broadcast)
    async fn forward_router_to_all_gcs(
        mut router_read: tokio::net::tcp::OwnedReadHalf,
        router_write: Arc<RwLock<tokio::net::tcp::OwnedWriteHalf>>,
        state: Arc<ProxyState>,
        rule_engine: Arc<RuleEngine>,
    ) -> Result<()> {
        info!("Router -> All GCS broadcast started");

        loop {
            // Read MAVLink packet from Router
            let packet = match read_mavlink_packet(&mut router_read).await {
                Ok(pkt) => pkt,
                Err(e) => {
                    error!("Error reading from router: {}", e);
                    break;
                }
            };

            debug!("Router -> All GCS: {} bytes", packet.len());

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

            // Send ACK if auto_ack is enabled (back to router)
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

            // Process actions and broadcast to all GCS clients
            // Note: For broadcast, we handle it specially since we need to send to multiple clients
            if result.actions.is_empty() || matches!(result.actions.first(), Some(Action::Forward)) {
                // Simple forward - just broadcast the packet
                state.broadcast_to_all_gcs(&packet).await;
            } else {
                // Complex actions (modify, delay, etc.) - process then broadcast
                // We'll create a custom destination that broadcasts
                execute_actions_impl_broadcast(
                    result.actions,
                    vec![packet],
                    state.clone(),
                )
                .await;
            }
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
