mod api;

use anyhow::{Context, Result};
use mlua::{Lua, Value};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, info, warn};

pub use api::PluginContext;

/// Plugin manager that handles loading and executing Lua scripts
pub struct PluginManager {
    lua: Arc<Lua>,
    plugins: HashMap<String, String>, // name -> lua code
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new() -> Result<Self> {
        let lua = Lua::new();

        // Initialize the Lua environment with our APIs
        api::init_lua_api(&lua)?;

        Ok(Self {
            lua: Arc::new(lua),
            plugins: HashMap::new(),
        })
    }

    /// Load a plugin from a file
    pub fn load_plugin(&mut self, name: &str, path: &Path) -> Result<()> {
        info!("Loading plugin '{}' from {:?}", name, path);

        let code = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read plugin file: {:?}", path))?;

        // Validate the plugin by compiling it
        self.lua
            .load(&code)
            .set_name(name)
            .exec()
            .map_err(|e| anyhow::anyhow!("Failed to compile plugin '{}': {}", name, e))?;

        self.plugins.insert(name.to_string(), code);

        debug!("Plugin '{}' loaded successfully", name);
        Ok(())
    }

    /// Execute a plugin's on_match function
    pub fn execute_plugin(&self, name: &str, context: &PluginContext) -> Result<()> {
        let code = self
            .plugins
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Plugin '{}' not found", name))?;

        // Create a fresh environment for this execution
        let globals = self.lua.globals();

        // Set the context table
        let context_table = self.lua.create_table()
            .map_err(|e| anyhow::anyhow!("Failed to create context table: {}", e))?;

        context_table.set("target_system", context.target_system)
            .map_err(|e| anyhow::anyhow!("Failed to set target_system: {}", e))?;
        context_table.set("target_component", context.target_component)
            .map_err(|e| anyhow::anyhow!("Failed to set target_component: {}", e))?;
        context_table.set("message_type", context.message_type.clone())
            .map_err(|e| anyhow::anyhow!("Failed to set message_type: {}", e))?;

        if let Some(cmd) = &context.command {
            context_table.set("command", cmd.clone())
                .map_err(|e| anyhow::anyhow!("Failed to set command: {}", e))?;
        }

        // Set parameters if available
        if let Some(params) = &context.params {
            let params_table = self.lua.create_table()
                .map_err(|e| anyhow::anyhow!("Failed to create params table: {}", e))?;
            for (i, value) in params.iter().enumerate() {
                params_table.set(i + 1, *value)
                    .map_err(|e| anyhow::anyhow!("Failed to set param {}: {}", i + 1, e))?;
            }
            context_table.set("params", params_table)
                .map_err(|e| anyhow::anyhow!("Failed to set params: {}", e))?;
        }

        globals.set("context", context_table)
            .map_err(|e| anyhow::anyhow!("Failed to set context global: {}", e))?;

        // Execute the plugin code
        self.lua
            .load(code)
            .set_name(name)
            .exec()
            .map_err(|e| anyhow::anyhow!("Failed to execute plugin '{}': {}", name, e))?;

        // Call on_match if it exists
        let on_match: Option<mlua::Function> = globals.get("on_match").ok();
        if let Some(on_match) = on_match {
            let ctx_val: Value = globals.get("context")
                .map_err(|e| anyhow::anyhow!("Failed to get context: {}", e))?;
            on_match
                .call::<()>(ctx_val)
                .map_err(|e| anyhow::anyhow!("Plugin '{}' on_match() failed: {}", name, e))?;
        } else {
            warn!("Plugin '{}' has no on_match() function", name);
        }

        Ok(())
    }

    /// Get list of loaded plugins
    #[allow(dead_code)]
    pub fn loaded_plugins(&self) -> Vec<String> {
        self.plugins.keys().cloned().collect()
    }
}
