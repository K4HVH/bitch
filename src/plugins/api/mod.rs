mod http;
mod log;
mod serial;
mod util;

use anyhow::Result;
use mlua::Lua;
use serde::{Deserialize, Serialize};

/// Context passed to plugins when a rule matches
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginContext {
    pub target_system: u8,
    pub target_component: u8,
    pub message_type: String,
    pub command: Option<String>,
    pub params: Option<Vec<f32>>,
}

/// Initialize all Lua APIs
pub fn init_lua_api(lua: &Lua) -> Result<()> {
    log::init(lua)?;
    serial::init(lua)?;
    http::init(lua)?;
    util::init(lua)?;

    Ok(())
}
