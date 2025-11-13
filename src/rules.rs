use crate::config::{CommandRule, RuleConditions};
use crate::modifiers::ModifierManager;
use crate::plugins::{PluginContext, PluginManager};
use anyhow::Result;
use mavlink::ardupilotmega::MavMessage;
use mavlink::MavHeader;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Information needed to send a generic ACK message
#[derive(Debug, Clone)]
pub struct AckInfo {
    /// The message type to send (e.g., "COMMAND_ACK", "MISSION_ACK")
    pub message_type: String,
    /// Source system_id for the ACK (extracted from matched message)
    pub source_system: u8,
    /// Source component_id for the ACK (extracted from matched message)
    pub source_component: u8,
    /// Fields to set in the ACK message (generic key-value pairs)
    pub fields: HashMap<String, toml::Value>,
    /// Fields to copy from original message (ACK field -> original field path)
    pub copy_fields: HashMap<String, String>,
    /// Original message header (for extracting GCS system/component IDs)
    pub original_header: MavHeader,
    /// Original message data as JSON (for extracting fields)
    pub original_message: JsonValue,
}

/// Result of processing a message through the rule engine
#[derive(Debug)]
pub struct ProcessResult {
    pub actions: Vec<Action>,
    pub ack_info: Option<AckInfo>,
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
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
        /// Optional: Field name in message to extract system_id from (e.g., "target_system")
        /// If None, uses header.system_id
        system_id_field: Option<String>,
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
    state_manager: Arc<crate::rule_state::RuleStateManager>,
}

impl RuleEngine {
    pub fn new(
        rules: Vec<CommandRule>,
        plugin_manager: PluginManager,
        modifier_manager: ModifierManager,
        state_manager: Arc<crate::rule_state::RuleStateManager>,
    ) -> Result<Self> {
        Ok(Self {
            rules,
            plugin_manager: Arc::new(plugin_manager),
            modifier_manager: Arc::new(modifier_manager),
            state_manager,
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
            // Check if rule is enabled
            if !self.state_manager.is_rule_enabled(&rule.name) {
                debug!("Rule '{}' is disabled, skipping", rule.name);
                continue;
            }

            if self.matches_rule(header, msg, rule, direction) {
                info!(
                    "Rule matched: '{}' - {}",
                    rule.name,
                    rule.description.as_deref().unwrap_or("no description")
                );

                // Execute triggers on_match if configured
                if let Some(triggers) = &rule.triggers {
                    if triggers.on_match {
                        self.execute_triggers(triggers, &rule.name);
                    }
                }

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

    /// Execute triggers (activate/deactivate other rules)
    fn execute_triggers(&self, triggers: &crate::config::TriggerConfig, source_rule: &str) {
        use std::time::Duration;

        // Activate rules
        for rule_name in &triggers.activate_rules {
            if let Some(duration_secs) = triggers.duration_seconds {
                let duration = Duration::from_secs(duration_secs);
                self.state_manager.activate_rule(rule_name, duration);
                info!(
                    "Rule '{}' activated rule '{}' for {}s",
                    source_rule, rule_name, duration_secs
                );
            }
        }

        // Deactivate rules
        for rule_name in &triggers.deactivate_rules {
            self.state_manager.deactivate_rule(rule_name);
            info!(
                "Rule '{}' deactivated rule '{}'",
                source_rule, rule_name
            );
        }
    }

    /// Build plugin context from MAVLINK message (works for all message types)
    fn build_plugin_context(&self, header: &MavHeader, msg: &MavMessage) -> PluginContext {
        let message_type = get_message_name(msg);

        // Serialize message to JSON (mavlink internally-tagged format)
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

        // Serialize message to JSON (mavlink internally-tagged format)
        let message_json = match serde_json::to_value(msg) {
            Ok(val) => val,
            Err(e) => {
                warn!("Failed to serialize message for condition checking: {}", e);
                return false;
            }
        };

        // Check conditions (fields accessed directly from internally-tagged format)
        if !self.matches_conditions(header, &message_json, &rule.conditions) {
            return false;
        }

        true
    }

    /// Check if conditions match for any message type
    fn matches_conditions(
        &self,
        header: &MavHeader,
        msg_json: &JsonValue,
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

        // Check all field conditions generically (works for ALL message types)
        // Fields accessed directly from internally-tagged format
        for (field_name, expected_value) in &conditions.custom {
            if !self.check_field_condition(msg_json, field_name, expected_value) {
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
                actual_value.as_str() == Some(expected)
            }
            toml::Value::Boolean(expected) => {
                actual_value.as_bool() == Some(*expected)
            }
            toml::Value::Table(_) => {
                // For tables (e.g., internally-tagged enums), convert to JSON and compare
                let expected_json = toml_to_json_value(expected_value);
                actual_value == &expected_json
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
        // Build ACK info if auto_ack is enabled (works for ANY message type)
        let ack_info = if rule.auto_ack {
            self.build_ack_info(rule, msg, header)
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
                    let system_id_field = rule.batch_system_id_field.clone();
                    Action::Batch {
                        count,
                        timeout,
                        key,
                        forward_on_timeout,
                        system_id_field,
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

    /// Build ACK info generically from any message type
    fn build_ack_info(&self, rule: &CommandRule, msg: &MavMessage, header: &MavHeader) -> Option<AckInfo> {
        // Get ACK config
        let ack_config = rule.ack.as_ref()?;

        let message_type = &ack_config.message_type;
        let source_system_field = &ack_config.source_system_field;
        let source_component_field = &ack_config.source_component_field;

        // Serialize message to JSON (mavlink internally-tagged format)
        let message_json = match serde_json::to_value(msg) {
            Ok(val) => val,
            Err(e) => {
                warn!("Failed to serialize message for ACK building: {}", e);
                return None;
            }
        };

        // Extract source system_id from specified field
        let source_system = match message_json.get(source_system_field) {
            Some(val) => match val.as_u64() {
                Some(v) => v as u8,
                None => {
                    warn!("Field '{}' is not a valid system_id", source_system_field);
                    return None;
                }
            },
            None => {
                warn!("Field '{}' not found in message for source_system", source_system_field);
                return None;
            }
        };

        // Extract source component_id from specified field
        let source_component = match message_json.get(source_component_field) {
            Some(val) => match val.as_u64() {
                Some(v) => v as u8,
                None => {
                    warn!("Field '{}' is not a valid component_id", source_component_field);
                    return None;
                }
            },
            None => {
                warn!("Field '{}' not found in message for source_component", source_component_field);
                return None;
            }
        };

        Some(AckInfo {
            message_type: message_type.clone(),
            source_system,
            source_component,
            fields: ack_config.fields.clone(),
            copy_fields: ack_config.copy_fields.clone(),
            original_header: *header,
            original_message: message_json,
        })
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

/// Convert TOML value to JSON value, preserving structure
fn toml_to_json_value(value: &toml::Value) -> JsonValue {
    match value {
        toml::Value::String(s) => JsonValue::String(s.clone()),
        toml::Value::Integer(i) => JsonValue::Number((*i).into()),
        toml::Value::Float(f) => {
            serde_json::Number::from_f64(*f)
                .map(JsonValue::Number)
                .unwrap_or(JsonValue::Null)
        }
        toml::Value::Boolean(b) => JsonValue::Bool(*b),
        toml::Value::Array(arr) => {
            let json_arr: Vec<JsonValue> = arr.iter().map(toml_to_json_value).collect();
            JsonValue::Array(json_arr)
        }
        toml::Value::Table(table) => {
            let mut json_obj = serde_json::Map::new();
            for (k, v) in table {
                json_obj.insert(k.clone(), toml_to_json_value(v));
            }
            JsonValue::Object(json_obj)
        }
        toml::Value::Datetime(dt) => JsonValue::String(dt.to_string()),
    }
}

