use crate::config::{CommandRule, RuleConditions};
use crate::modifiers::ModifierManager;
use crate::plugins::{PluginContext, PluginManager};
use anyhow::Result;
use mavlink::ardupilotmega::MavMessage;
use mavlink::MavHeader;
use serde_json::Value as JsonValue;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Information needed to send a COMMAND_ACK
#[derive(Debug, Clone)]
pub struct AckInfo {
    pub command: mavlink::ardupilotmega::MavCmd,
    pub target_system: u8,
    pub target_component: u8,
}

/// Result of processing a message through the rule engine
#[derive(Debug)]
pub struct ProcessResult {
    pub actions: Vec<Action>,
    pub ack_info: Option<AckInfo>,
}

#[derive(Debug, Clone)]
pub enum Action {
    /// Forward the message immediately
    Forward,
    /// Delay the message by the specified duration
    Delay(Duration),
    /// Block the message completely
    Block,
    /// Batch messages until threshold is met or timeout occurs
    Batch {
        count: usize,
        timeout: Duration,
        key: String,
        forward_on_timeout: bool,
    },
    /// Modify the message using a Lua modifier script
    Modify {
        modifier: String,
        modified_message: Option<MavMessage>,
    },
}

/// Rule engine for processing MAVLINK messages
pub struct RuleEngine {
    rules: Vec<CommandRule>,
    plugin_manager: Arc<PluginManager>,
    modifier_manager: Arc<ModifierManager>,
}

impl RuleEngine {
    pub fn new(
        rules: Vec<CommandRule>,
        plugin_manager: PluginManager,
        modifier_manager: ModifierManager,
    ) -> Result<Self> {
        Ok(Self {
            rules,
            plugin_manager: Arc::new(plugin_manager),
            modifier_manager: Arc::new(modifier_manager),
        })
    }

    /// Process a MAVLINK message and return the appropriate action
    /// Defaults to "gcs_to_router" direction for backward compatibility
    #[allow(dead_code)]
    pub fn process_message(&self, header: &MavHeader, msg: &MavMessage) -> ProcessResult {
        self.process_message_with_direction(header, msg, "gcs_to_router")
    }

    /// Process a MAVLINK message with a specified direction filter
    pub fn process_message_with_direction(
        &self,
        header: &MavHeader,
        msg: &MavMessage,
        direction: &str,
    ) -> ProcessResult {
        let msg_name = get_message_name(msg);
        debug!(
            "Processing message: sysid={}, compid={}, msg={}, direction={}",
            header.system_id, header.component_id, msg_name, direction
        );

        // Find the first matching rule (rules are sorted by priority)
        for rule in &self.rules {
            if self.matches_rule(header, msg, rule, direction) {
                info!(
                    "Rule matched: {} {} - {}",
                    rule.message_type,
                    rule.command
                        .as_ref()
                        .map(|cmd| format!("({})", cmd))
                        .unwrap_or_default(),
                    rule.description.as_deref().unwrap_or("no description")
                );

                // Execute plugins for this rule
                self.execute_plugins(rule, header, msg);

                return self.execute_action(rule, msg, header);
            }
        }

        // No rule matched, forward by default
        ProcessResult {
            actions: vec![Action::Forward],
            ack_info: None,
        }
    }

    /// Execute all plugins attached to a rule
    fn execute_plugins(&self, rule: &CommandRule, header: &MavHeader, msg: &MavMessage) {
        if rule.plugins.is_empty() {
            return;
        }

        // Build context for plugins
        let context = self.build_plugin_context(header, msg);

        // Execute each plugin
        for plugin_name in &rule.plugins {
            if let Err(e) = self.plugin_manager.execute_plugin(plugin_name, &context) {
                warn!("Plugin '{}' execution failed: {}", plugin_name, e);
            }
        }
    }

    /// Build plugin context from MAVLINK message (works for all message types)
    fn build_plugin_context(&self, header: &MavHeader, msg: &MavMessage) -> PluginContext {
        let message_type = get_message_name(msg);

        // Serialize the full message to JSON
        let message_json = serde_json::to_value(msg)
            .unwrap_or_else(|_| serde_json::json!({}));

        PluginContext {
            system_id: header.system_id,
            component_id: header.component_id,
            message_type,
            message: message_json,
        }
    }

    /// Check if a message matches a specific rule (works for all message types)
    fn matches_rule(&self, header: &MavHeader, msg: &MavMessage, rule: &CommandRule, direction: &str) -> bool {
        // Check direction filter first
        if rule.direction != "both" && rule.direction != direction {
            return false;
        }

        // Check message type
        let msg_name = get_message_name(msg);
        if rule.message_type != msg_name {
            return false;
        }

        // Serialize message to JSON for generic field access
        let msg_json = match serde_json::to_value(msg) {
            Ok(val) => val,
            Err(e) => {
                warn!("Failed to serialize message for condition checking: {}", e);
                return false;
            }
        };

        // For COMMAND_LONG, check command name if specified
        if rule.message_type == "COMMAND_LONG" && rule.command.is_some() {
            if let Some(cmd_obj) = msg_json.get("COMMAND_LONG") {
                if let Some(cmd_field) = cmd_obj.get("command") {
                    let cmd_str = format!("{:?}", cmd_field);
                    if let Some(ref expected_cmd) = rule.command {
                        if !cmd_str.contains(expected_cmd) {
                            debug!("Command mismatch: expected {}, got {}", expected_cmd, cmd_str);
                            return false;
                        }
                    }
                }
            }
        }

        // Check conditions
        if !self.matches_conditions(header, &msg_json, &msg_name, &rule.conditions) {
            return false;
        }

        true
    }

    /// Check if conditions match for any message type
    fn matches_conditions(
        &self,
        header: &MavHeader,
        msg_json: &JsonValue,
        msg_type: &str,
        conditions: &RuleConditions,
    ) -> bool {
        // Check header conditions (work for all message types)
        if let Some(expected_sysid) = conditions.system_id {
            if header.system_id != expected_sysid {
                debug!("System ID mismatch: expected {}, got {}", expected_sysid, header.system_id);
                return false;
            }
        }

        if let Some(expected_compid) = conditions.component_id {
            if header.component_id != expected_compid {
                debug!("Component ID mismatch: expected {}, got {}", expected_compid, header.component_id);
                return false;
            }
        }

        // Get the message data object (e.g., msg_json["COMMAND_LONG"])
        let msg_data = match msg_json.get(msg_type) {
            Some(data) => data,
            None => {
                debug!("Message data not found for type {}", msg_type);
                return true; // No message-specific conditions to check
            }
        };

        // Check COMMAND_LONG param conditions (explicit fields)
        if msg_type == "COMMAND_LONG" {
            if !self.check_param_condition(msg_data, "param1", conditions.param1) {
                return false;
            }
            if !self.check_param_condition(msg_data, "param2", conditions.param2) {
                return false;
            }
            if !self.check_param_condition(msg_data, "param3", conditions.param3) {
                return false;
            }
            if !self.check_param_condition(msg_data, "param4", conditions.param4) {
                return false;
            }
            if !self.check_param_condition(msg_data, "param5", conditions.param5) {
                return false;
            }
            if !self.check_param_condition(msg_data, "param6", conditions.param6) {
                return false;
            }
            if !self.check_param_condition(msg_data, "param7", conditions.param7) {
                return false;
            }
        }

        // Check any other field conditions (works for ALL message types)
        for (field_name, expected_value) in &conditions.custom {
            if !self.check_field_condition(msg_data, field_name, expected_value) {
                return false;
            }
        }

        true
    }

    /// Check a single parameter condition
    fn check_param_condition(&self, msg_data: &JsonValue, field: &str, expected: Option<f32>) -> bool {
        if let Some(expected_val) = expected {
            if let Some(actual) = msg_data.get(field).and_then(|v| v.as_f64()) {
                if (actual as f32 - expected_val).abs() > f32::EPSILON {
                    debug!("{} mismatch: expected {}, got {}", field, expected_val, actual);
                    return false;
                }
            } else {
                debug!("{} field not found or not a number", field);
                return false;
            }
        }
        true
    }

    /// Check a field condition (works for any field in any message type)
    fn check_field_condition(&self, msg_data: &JsonValue, field_name: &str, expected_value: &toml::Value) -> bool {
        // Get the actual field value from the message
        let actual_value = match msg_data.get(field_name) {
            Some(val) => val,
            None => {
                debug!("Field '{}' not found in message", field_name);
                return false;
            }
        };

        // Convert TOML value to comparable format
        let matches = match expected_value {
            toml::Value::Integer(expected) => {
                actual_value.as_i64() == Some(*expected)
            }
            toml::Value::Float(expected) => {
                if let Some(actual) = actual_value.as_f64() {
                    (actual - *expected).abs() < f64::EPSILON
                } else {
                    false
                }
            }
            toml::Value::String(expected) => {
                // For string matching, handle both exact match and contains
                if let Some(actual) = actual_value.as_str() {
                    actual == expected || actual.contains(expected)
                } else {
                    // Also check if it's an enum/object that contains the string
                    format!("{:?}", actual_value).contains(expected)
                }
            }
            toml::Value::Boolean(expected) => {
                actual_value.as_bool() == Some(*expected)
            }
            _ => {
                debug!("Unsupported condition value type for field '{}'", field_name);
                false
            }
        };

        if !matches {
            debug!("Condition mismatch for '{}': expected {:?}, got {:?}", field_name, expected_value, actual_value);
        }

        matches
    }

    /// Execute the action sequence specified by a rule
    fn execute_action(&self, rule: &CommandRule, msg: &MavMessage, header: &MavHeader) -> ProcessResult {
        // Build ACK info if auto_ack is enabled and this is a COMMAND_LONG
        let ack_info = if rule.auto_ack {
            match msg {
                MavMessage::COMMAND_LONG(cmd) => Some(AckInfo {
                    command: cmd.command,
                    target_system: cmd.target_system,
                    target_component: cmd.target_component,
                }),
                _ => {
                    warn!("auto_ack enabled but message is not COMMAND_LONG, ignoring");
                    None
                }
            }
        } else {
            None
        };

        // Build action sequence from rule
        let action_names = rule.get_actions();
        let mut actions = Vec::new();

        for action_name in action_names {
            let action = match action_name.as_str() {
                "delay" => {
                    let delay = Duration::from_secs(rule.delay_seconds.unwrap_or(0));
                    Action::Delay(delay)
                }
                "batch" => {
                    let count = rule.batch_count.unwrap_or(1);
                    let timeout = Duration::from_secs(rule.batch_timeout_seconds.unwrap_or(30));
                    let key = rule.batch_key.clone();
                    let forward_on_timeout = rule.batch_timeout_forward;
                    Action::Batch {
                        count,
                        timeout,
                        key,
                        forward_on_timeout,
                    }
                }
                "block" => Action::Block,
                "forward" => Action::Forward,
                "modify" => {
                    if let Some(ref modifier_name) = rule.modifier {
                        // Execute the modifier with the full message
                        match self.modifier_manager.execute_modifier(modifier_name, header, msg) {
                            Ok(modified_msg) => {
                                Action::Modify {
                                    modifier: modifier_name.clone(),
                                    modified_message: Some(modified_msg),
                                }
                            }
                            Err(e) => {
                                warn!("Modifier '{}' execution failed: {}", modifier_name, e);
                                Action::Forward
                            }
                        }
                    } else {
                        warn!("Modify action specified but no modifier configured");
                        Action::Forward
                    }
                }
                _ => {
                    info!("Unknown action '{}', using forward", action_name);
                    Action::Forward
                }
            };
            actions.push(action);
        }

        ProcessResult { actions, ack_info }
    }
}

/// Parse a MAVLINK message from raw packet data
pub fn parse_mavlink_message(packet: &[u8]) -> Result<(MavHeader, MavMessage)> {
    use mavlink::peek_reader::PeekReader;
    use std::io::Cursor;

    let cursor = Cursor::new(packet);
    let mut peek_reader = PeekReader::new(cursor);

    // Try to read as MAVLink v2 message
    match mavlink::read_v2_msg::<MavMessage, _>(&mut peek_reader) {
        Ok((header, msg)) => Ok((header, msg)),
        Err(_) => {
            // Try v1 if v2 fails
            let cursor = Cursor::new(packet);
            let mut peek_reader = PeekReader::new(cursor);
            mavlink::read_v1_msg::<MavMessage, _>(&mut peek_reader)
                .map_err(|e| anyhow::anyhow!("Failed to parse MAVLink: {:?}", e))
        }
    }
}

/// Get the name of a MAVLINK message enum variant as a string
pub fn get_message_name(msg: &MavMessage) -> String {
    let debug_str = format!("{:?}", msg);
    // Extract just the variant name (before the '(')
    debug_str
        .split('(')
        .next()
        .unwrap_or("UNKNOWN")
        .to_string()
}

