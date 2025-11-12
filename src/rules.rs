use crate::config::{CommandRule, RuleConditions};
use crate::plugins::{PluginContext, PluginManager};
use anyhow::Result;
use mavlink::ardupilotmega::MavMessage;
use mavlink::MavHeader;
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
    pub action: Action,
    pub ack_info: Option<AckInfo>,
}

#[derive(Debug)]
pub enum Action {
    /// Forward the message immediately
    Forward,
    /// Delay the message by the specified duration
    Delay(Duration),
    /// Block the message completely
    Block,
    // Future: Modify could include modified packet data
    // Modify(Vec<u8>),
}

/// Rule engine for processing MAVLINK messages
pub struct RuleEngine {
    rules: Vec<CommandRule>,
    plugin_manager: Arc<PluginManager>,
}

impl RuleEngine {
    pub fn new(rules: Vec<CommandRule>, plugin_manager: PluginManager) -> Result<Self> {
        Ok(Self {
            rules,
            plugin_manager: Arc::new(plugin_manager),
        })
    }

    /// Process a MAVLINK message and return the appropriate action
    pub fn process_message(&self, header: &MavHeader, msg: &MavMessage) -> ProcessResult {
        let msg_name = get_message_name(msg);
        debug!(
            "Processing message: sysid={}, compid={}, msg={}",
            header.system_id, header.component_id, msg_name
        );

        // Find the first matching rule (rules are sorted by priority)
        for rule in &self.rules {
            if self.matches_rule(header, msg, rule) {
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

                return self.execute_action(rule, msg);
            }
        }

        // No rule matched, forward by default
        ProcessResult {
            action: Action::Forward,
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

    /// Build plugin context from MAVLINK message
    fn build_plugin_context(&self, header: &MavHeader, msg: &MavMessage) -> PluginContext {
        let message_type = get_message_name(msg);

        let (target_system, target_component, command, params) = match msg {
            MavMessage::COMMAND_LONG(cmd) => {
                let command_name = get_command_name(&cmd.command);
                let params = vec![
                    cmd.param1, cmd.param2, cmd.param3, cmd.param4,
                    cmd.param5, cmd.param6, cmd.param7,
                ];
                (cmd.target_system, cmd.target_component, Some(command_name), Some(params))
            }
            _ => (header.system_id, header.component_id, None, None),
        };

        PluginContext {
            target_system,
            target_component,
            message_type,
            command,
            params,
        }
    }

    /// Check if a message matches a specific rule
    fn matches_rule(&self, header: &MavHeader, msg: &MavMessage, rule: &CommandRule) -> bool {
        // Check message type
        let msg_name = get_message_name(msg);
        if rule.message_type != msg_name {
            return false;
        }

        // For COMMAND_LONG messages, check command name and parameters
        if rule.message_type == "COMMAND_LONG" {
            if let MavMessage::COMMAND_LONG(cmd) = msg {
                // Check command name if specified
                if let Some(ref expected_cmd) = rule.command {
                    let actual_cmd = get_command_name(&cmd.command);
                    if actual_cmd != *expected_cmd {
                        debug!("Command mismatch: expected {}, got {}", expected_cmd, actual_cmd);
                        return false;
                    }
                }

                // Check conditions
                if !self.matches_conditions(header, cmd, &rule.conditions) {
                    return false;
                }

                return true;
            }
        }

        // Add support for other message types here
        // For now, if we reach here, the message type matched but no specific handler exists
        false
    }

    /// Check if conditions match for a COMMAND_LONG message
    fn matches_conditions(
        &self,
        header: &MavHeader,
        cmd: &mavlink::ardupilotmega::COMMAND_LONG_DATA,
        conditions: &RuleConditions,
    ) -> bool {
        // Check system_id
        if let Some(expected_sysid) = conditions.system_id {
            if header.system_id != expected_sysid {
                debug!("System ID mismatch: expected {}, got {}", expected_sysid, header.system_id);
                return false;
            }
        }

        // Check component_id
        if let Some(expected_compid) = conditions.component_id {
            if header.component_id != expected_compid {
                debug!("Component ID mismatch: expected {}, got {}", expected_compid, header.component_id);
                return false;
            }
        }

        // Check parameters
        if let Some(expected) = conditions.param1 {
            if (cmd.param1 - expected).abs() > f32::EPSILON {
                debug!("param1 mismatch: expected {}, got {}", expected, cmd.param1);
                return false;
            }
        }

        if let Some(expected) = conditions.param2 {
            if (cmd.param2 - expected).abs() > f32::EPSILON {
                debug!("param2 mismatch: expected {}, got {}", expected, cmd.param2);
                return false;
            }
        }

        if let Some(expected) = conditions.param3 {
            if (cmd.param3 - expected).abs() > f32::EPSILON {
                debug!("param3 mismatch: expected {}, got {}", expected, cmd.param3);
                return false;
            }
        }

        if let Some(expected) = conditions.param4 {
            if (cmd.param4 - expected).abs() > f32::EPSILON {
                debug!("param4 mismatch: expected {}, got {}", expected, cmd.param4);
                return false;
            }
        }

        if let Some(expected) = conditions.param5 {
            if (cmd.param5 - expected).abs() > f32::EPSILON {
                debug!("param5 mismatch: expected {}, got {}", expected, cmd.param5);
                return false;
            }
        }

        if let Some(expected) = conditions.param6 {
            if (cmd.param6 - expected).abs() > f32::EPSILON {
                debug!("param6 mismatch: expected {}, got {}", expected, cmd.param6);
                return false;
            }
        }

        if let Some(expected) = conditions.param7 {
            if (cmd.param7 - expected).abs() > f32::EPSILON {
                debug!("param7 mismatch: expected {}, got {}", expected, cmd.param7);
                return false;
            }
        }

        true
    }

    /// Execute the action specified by a rule
    fn execute_action(&self, rule: &CommandRule, msg: &MavMessage) -> ProcessResult {
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

        let action = match rule.action.as_str() {
            "delay" => {
                let delay = Duration::from_secs(rule.delay_seconds.unwrap_or(0));
                Action::Delay(delay)
            }
            "block" => Action::Block,
            "forward" => Action::Forward,
            "modify" => {
                // Future: implement message modification
                info!("Modify action not yet implemented, forwarding");
                Action::Forward
            }
            _ => {
                info!("Unknown action '{}', forwarding", rule.action);
                Action::Forward
            }
        };

        ProcessResult { action, ack_info }
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

/// Get the name of a MAVLink command enum variant as a string
fn get_command_name(cmd: &mavlink::ardupilotmega::MavCmd) -> String {
    format!("{:?}", cmd)
}
