use anyhow::{Context, Result};
use mavlink::ardupilotmega::MavMessage;
use mavlink::MavHeader;
use mlua::{Lua, LuaSerdeExt, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};
use serde_json::Value as JsonValue;

/// Manager for loading and executing Lua modifier scripts
pub struct ModifierManager {
    lua: Arc<Lua>,
    modifiers: HashMap<String, String>, // name -> lua code
}

impl ModifierManager {
    /// Create a new modifier manager
    pub fn new() -> Result<Self> {
        let lua = Lua::new();

        // Initialize the Lua environment with logging API
        Self::init_lua_api(&lua)?;

        Ok(Self {
            lua: Arc::new(lua),
            modifiers: HashMap::new(),
        })
    }

    /// Initialize Lua APIs available to modifiers
    fn init_lua_api(lua: &Lua) -> Result<()> {
        // Import log API for modifiers to use
        let log_table = lua.create_table()?;

        let info = lua.create_function(|_, msg: String| {
            tracing::info!("[Modifier] {}", msg);
            Ok(())
        })?;
        log_table.set("info", info)?;

        let warn = lua.create_function(|_, msg: String| {
            tracing::warn!("[Modifier] {}", msg);
            Ok(())
        })?;
        log_table.set("warn", warn)?;

        let error = lua.create_function(|_, msg: String| {
            tracing::error!("[Modifier] {}", msg);
            Ok(())
        })?;
        log_table.set("error", error)?;

        let debug = lua.create_function(|_, msg: String| {
            tracing::debug!("[Modifier] {}", msg);
            Ok(())
        })?;
        log_table.set("debug", debug)?;

        lua.globals().set("log", log_table)?;

        Ok(())
    }

    /// Load a modifier from a file
    pub fn load_modifier(&mut self, name: &str, path: &Path) -> Result<()> {
        info!("Loading modifier '{}' from {:?}", name, path);

        let code = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read modifier file: {:?}", path))?;

        // Validate the modifier by compiling it
        self.lua
            .load(&code)
            .set_name(name)
            .exec()
            .map_err(|e| anyhow::anyhow!("Failed to compile modifier '{}': {}", name, e))?;

        self.modifiers.insert(name.to_string(), code);

        debug!("Modifier '{}' loaded successfully", name);
        Ok(())
    }

    /// Execute a modifier and return the modified message
    pub fn execute_modifier(
        &self,
        name: &str,
        header: &MavHeader,
        msg: &MavMessage,
        trigger_context: &HashMap<String, JsonValue>,
    ) -> Result<MavMessage> {
        let code = self
            .modifiers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Modifier '{}' not found", name))?;

        let globals = self.lua.globals();

        // Get message type name
        let message_type = format!("{:?}", msg).split('(').next().unwrap_or("UNKNOWN").to_string();

        // Create a context table with header and message data
        let context_table = self.lua.create_table()
            .map_err(|e| anyhow::anyhow!("Failed to create context table: {}", e))?;

        // Add header fields
        context_table.set("system_id", header.system_id)
            .map_err(|e| anyhow::anyhow!("Failed to set system_id: {}", e))?;
        context_table.set("component_id", header.component_id)
            .map_err(|e| anyhow::anyhow!("Failed to set component_id: {}", e))?;
        context_table.set("sequence", header.sequence)
            .map_err(|e| anyhow::anyhow!("Failed to set sequence: {}", e))?;
        context_table.set("message_type", message_type.as_str())
            .map_err(|e| anyhow::anyhow!("Failed to set message_type: {}", e))?;

        // Serialize message to JSON (mavlink internally-tagged format)
        let message_json = serde_json::to_value(msg)
            .map_err(|e| anyhow::anyhow!("Failed to serialize message to JSON: {}", e))?;

        // Convert JSON value to Lua value
        let msg_value = self.lua.to_value(&message_json)
            .map_err(|e| anyhow::anyhow!("Failed to serialize message to Lua: {}", e))?;

        context_table.set("message", msg_value)
            .map_err(|e| anyhow::anyhow!("Failed to set message: {}", e))?;

        // Add trigger_context if present
        if !trigger_context.is_empty() {
            let trigger_ctx_value = self.lua.to_value(trigger_context)
                .map_err(|e| anyhow::anyhow!("Failed to serialize trigger_context to Lua: {}", e))?;
            context_table.set("trigger_context", trigger_ctx_value)
                .map_err(|e| anyhow::anyhow!("Failed to set trigger_context: {}", e))?;
        }

        globals.set("context", context_table)
            .map_err(|e| anyhow::anyhow!("Failed to set context global: {}", e))?;

        // Execute the modifier code
        self.lua
            .load(code)
            .set_name(name)
            .exec()
            .map_err(|e| anyhow::anyhow!("Failed to execute modifier '{}': {}", name, e))?;

        // Call modify function if it exists
        let modify_fn: Option<mlua::Function> = globals.get("modify").ok();
        if let Some(modify_fn) = modify_fn {
            let ctx_val: Value = globals
                .get("context")
                .map_err(|e| anyhow::anyhow!("Failed to get context: {}", e))?;

            let result_ctx = modify_fn
                .call::<Value>(ctx_val)
                .map_err(|e| anyhow::anyhow!("Modifier '{}' modify() failed: {}", name, e))?;

            // Extract modified message from returned context
            if let Value::Table(result_table) = result_ctx {
                let modified_msg_value: Value = result_table.get("message")
                    .map_err(|e| anyhow::anyhow!("Failed to get modified message: {}", e))?;

                // Convert Lua value to JSON
                let message_json: serde_json::Value = self.lua.from_value(modified_msg_value)
                    .map_err(|e| anyhow::anyhow!("Failed to convert modified message to JSON: {}", e))?;

                // Deserialize JSON to MavMessage (mavlink internally-tagged format)
                let modified_msg: MavMessage = serde_json::from_value(message_json)
                    .map_err(|e| anyhow::anyhow!("Failed to deserialize modified message: {}", e))?;

                Ok(modified_msg)
            } else {
                anyhow::bail!("Modifier '{}' modify() must return a table", name);
            }
        } else {
            warn!("Modifier '{}' has no modify() function", name);
            Ok(msg.clone())
        }
    }

    /// Get list of loaded modifiers
    #[allow(dead_code)]
    pub fn loaded_modifiers(&self) -> Vec<String> {
        self.modifiers.keys().cloned().collect()
    }
}
