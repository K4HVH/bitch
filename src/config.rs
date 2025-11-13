use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub network: NetworkConfig,
    pub logging: LoggingConfig,
    #[serde(default)]
    pub plugins: PluginsConfig,
    #[serde(default)]
    pub modifiers: ModifiersConfig,
    #[serde(default)]
    pub rules: Vec<CommandRule>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct PluginsConfig {
    /// Directory containing plugin files
    #[serde(default = "default_plugins_dir")]
    pub directory: String,
    /// List of plugins to load (name -> filename)
    #[serde(default)]
    pub load: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct ModifiersConfig {
    /// Directory containing modifier files
    #[serde(default = "default_modifiers_dir")]
    pub directory: String,
    /// List of modifiers to load (name -> filename)
    #[serde(default)]
    pub load: HashMap<String, String>,
}

fn default_plugins_dir() -> String {
    "plugins".to_string()
}

fn default_modifiers_dir() -> String {
    "modifiers".to_string()
}

fn default_batch_timeout_forward() -> bool {
    true
}

fn default_batch_key() -> String {
    "default".to_string()
}

fn default_direction() -> String {
    "gcs_to_router".to_string()
}

#[derive(Debug, Deserialize, Clone)]
pub struct NetworkConfig {
    pub gcs_listen_port: u16,
    pub gcs_listen_address: String,
    pub router_address: String,
    pub router_port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LoggingConfig {
    pub level: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct AutoAckConfig {
    /// Message type to send as ACK (e.g., "COMMAND_ACK", "MISSION_ACK")
    pub message_type: String,

    /// Field name in matched message to use as ACK source system_id (e.g., "target_system")
    pub source_system_field: String,

    /// Field name in matched message to use as ACK source component_id (e.g., "target_component")
    pub source_component_field: String,

    /// Fields to set in ACK message (generic key-value pairs)
    #[serde(default)]
    pub fields: HashMap<String, toml::Value>,

    /// Map of ACK field names to original message field names to copy
    /// Example: {"command" = "command"}
    /// Use "header.X" to copy from original message header
    #[serde(default)]
    pub copy_fields: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommandRule {
    /// The type of MAVLINK message (e.g., "COMMAND_LONG", "MISSION_ITEM")
    pub message_type: String,

    /// Optional: Conditions that must match for this rule to apply
    #[serde(default)]
    pub conditions: RuleConditions,

    /// The action to take: "delay", "block", "forward", "modify", "batch"
    /// DEPRECATED: Use `actions` array instead for sequential actions
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,

    /// Sequential actions to apply (e.g., ["batch", "delay"])
    /// If not specified, will use single `action` field for backward compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>,

    /// Optional: Delay duration in seconds (for action = "delay")
    pub delay_seconds: Option<u64>,

    /// Optional: Number of unique system IDs to wait for (for action = "batch")
    pub batch_count: Option<usize>,

    /// Optional: Timeout in seconds for batch completion (for action = "batch")
    pub batch_timeout_seconds: Option<u64>,

    /// Optional: Whether to forward messages on timeout (for action = "batch")
    /// If false, messages are dropped on timeout. Default: true
    #[serde(default = "default_batch_timeout_forward")]
    pub batch_timeout_forward: bool,

    /// Optional: Batch group key (for action = "batch")
    /// Allows multiple independent batch groups. Default: "default"
    #[serde(default = "default_batch_key")]
    pub batch_key: String,

    /// Optional: Field name in message to use as batch system_id (e.g., "target_system")
    /// If not specified, uses header.system_id. Works for ANY message type.
    pub batch_system_id_field: Option<String>,

    /// Optional: List of plugins to execute when this rule matches
    #[serde(default)]
    pub plugins: Vec<String>,

    /// Optional: Automatically send ACK response to GCS (works for ANY message type)
    #[serde(default)]
    pub auto_ack: bool,

    /// Optional: ACK configuration (all ACK settings in one place)
    pub ack: Option<AutoAckConfig>,

    /// Optional: Lua modifier script name (for action = "modify")
    pub modifier: Option<String>,

    /// Optional: Human-readable description
    pub description: Option<String>,

    /// Optional: Priority (higher = checked first). Default: 0
    #[serde(default)]
    pub priority: i32,

    /// Optional: Message flow direction this rule applies to
    /// "gcs_to_router" (default), "router_to_gcs", or "both"
    #[serde(default = "default_direction")]
    pub direction: String,
}

impl CommandRule {
    /// Get the normalized actions array (handles backward compatibility)
    pub fn get_actions(&self) -> Vec<String> {
        if let Some(ref actions) = self.actions {
            actions.clone()
        } else if let Some(ref action) = self.action {
            vec![action.clone()]
        } else {
            vec![]
        }
    }
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RuleConditions {
    /// Match specific system IDs
    pub system_id: Option<u8>,

    /// Match specific component IDs
    pub component_id: Option<u8>,

    /// Generic field conditions - works for ALL message types
    /// Example: param1 = 1.0, altitude = 100, fix_type = 3, etc.
    #[serde(flatten)]
    pub custom: HashMap<String, toml::Value>,
}

impl Config {
    pub fn load(path: &str) -> Result<Self> {
        let contents = fs::read_to_string(path)
            .context(format!("Failed to read config file: {}", path))?;

        let mut config: Config = toml::from_str(&contents)
            .context("Failed to parse config file")?;

        // Sort rules by priority (highest first)
        config.rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        // Validate network config
        if self.network.gcs_listen_port == 0 {
            anyhow::bail!("gcs_listen_port must be greater than 0");
        }

        // Validate rules
        for (idx, rule) in self.rules.iter().enumerate() {
            let actions = rule.get_actions();

            // Ensure at least one action is specified
            if actions.is_empty() {
                anyhow::bail!("Rule {} has no action or actions specified", idx);
            }

            // Validate each action
            for action in &actions {
                if !["delay", "block", "forward", "modify", "batch"].contains(&action.as_str()) {
                    anyhow::bail!(
                        "Rule {} has invalid action '{}'. Must be: delay, block, forward, modify, or batch",
                        idx,
                        action
                    );
                }
            }

            // Validate direction field
            if !["gcs_to_router", "router_to_gcs", "both"].contains(&rule.direction.as_str()) {
                anyhow::bail!(
                    "Rule {} has invalid direction '{}'. Must be: gcs_to_router, router_to_gcs, or both",
                    idx,
                    rule.direction
                );
            }

            // Validate action-specific requirements
            if actions.contains(&"delay".to_string()) && rule.delay_seconds.is_none() {
                anyhow::bail!(
                    "Rule {} has 'delay' action but no delay_seconds specified",
                    idx
                );
            }

            if actions.contains(&"batch".to_string()) {
                if rule.batch_count.is_none() {
                    anyhow::bail!(
                        "Rule {} has 'batch' action but no batch_count specified",
                        idx
                    );
                }
                if rule.batch_timeout_seconds.is_none() {
                    anyhow::bail!(
                        "Rule {} has 'batch' action but no batch_timeout_seconds specified",
                        idx
                    );
                }
            }

            if actions.contains(&"modify".to_string()) && rule.modifier.is_none() {
                anyhow::bail!(
                    "Rule {} has 'modify' action but no modifier specified",
                    idx
                );
            }

            // Validate auto_ack requirements
            if rule.auto_ack && rule.ack.is_none() {
                anyhow::bail!(
                    "Rule {} has auto_ack enabled but no [rules.ack] section specified",
                    idx
                );
            }
        }

        Ok(())
    }
}
