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

fn default_plugins_dir() -> String {
    "plugins".to_string()
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
pub struct CommandRule {
    /// The type of MAVLINK message (e.g., "COMMAND_LONG", "MISSION_ITEM")
    pub message_type: String,

    /// Optional: The specific command name (for COMMAND_LONG)
    /// e.g., "MAV_CMD_COMPONENT_ARM_DISARM", "MAV_CMD_NAV_TAKEOFF"
    pub command: Option<String>,

    /// Optional: Conditions that must match for this rule to apply
    #[serde(default)]
    pub conditions: RuleConditions,

    /// The action to take: "delay", "block", "forward", "modify"
    pub action: String,

    /// Optional: Delay duration in seconds (for action = "delay")
    pub delay_seconds: Option<u64>,

    /// Optional: List of plugins to execute when this rule matches
    #[serde(default)]
    pub plugins: Vec<String>,

    /// Optional: Human-readable description
    pub description: Option<String>,

    /// Optional: Priority (higher = checked first). Default: 0
    #[serde(default)]
    pub priority: i32,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub struct RuleConditions {
    /// Match specific parameter values (e.g., param1 = 1.0 for ARM)
    pub param1: Option<f32>,
    pub param2: Option<f32>,
    pub param3: Option<f32>,
    pub param4: Option<f32>,
    pub param5: Option<f32>,
    pub param6: Option<f32>,
    pub param7: Option<f32>,

    /// Match specific system IDs
    pub system_id: Option<u8>,

    /// Match specific component IDs
    pub component_id: Option<u8>,

    /// Custom conditions (future use)
    #[serde(flatten)]
    #[allow(dead_code)]
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
            if !["delay", "block", "forward", "modify"].contains(&rule.action.as_str()) {
                anyhow::bail!(
                    "Rule {} has invalid action '{}'. Must be: delay, block, forward, or modify",
                    idx,
                    rule.action
                );
            }

            if rule.action == "delay" && rule.delay_seconds.is_none() {
                anyhow::bail!(
                    "Rule {} has action 'delay' but no delay_seconds specified",
                    idx
                );
            }
        }

        Ok(())
    }
}
