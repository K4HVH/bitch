mod http;
mod log;
mod serial;
mod util;

use anyhow::Result;
use mlua::Lua;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Context passed to plugins when a rule matches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    /// MAVLink header system ID
    pub system_id: u8,
    /// MAVLink header component ID
    pub component_id: u8,
    /// Message type name (e.g., "COMMAND_LONG", "HEARTBEAT")
    pub message_type: String,
    /// Full message data (works for ALL message types)
    pub message: JsonValue,
    /// Trigger context data (if rule was activated by a trigger)
    #[serde(default)]
    pub trigger_context: HashMap<String, JsonValue>,
}

/// Initialize all Lua APIs
pub fn init_lua_api(lua: &Lua) -> Result<()> {
    log::init(lua)?;
    serial::init(lua)?;
    http::init(lua)?;
    util::init(lua)?;

    Ok(())
}
