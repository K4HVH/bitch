use anyhow::Result;
use mlua::Lua;
use tracing::{debug, error, info, warn};

/// Initialize logging API for Lua
pub fn init(lua: &Lua) -> Result<()> {
    let log_table = lua.create_table()
        .map_err(|e| anyhow::anyhow!("Failed to create log table: {}", e))?;

    // log.info(message)
    log_table.set(
        "info",
        lua.create_function(|_, msg: String| {
            info!("[Plugin] {}", msg);
            Ok(())
        }).map_err(|e| anyhow::anyhow!("Failed to create log.info: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set log.info: {}", e))?;

    // log.warn(message)
    log_table.set(
        "warn",
        lua.create_function(|_, msg: String| {
            warn!("[Plugin] {}", msg);
            Ok(())
        }).map_err(|e| anyhow::anyhow!("Failed to create log.warn: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set log.warn: {}", e))?;

    // log.error(message)
    log_table.set(
        "error",
        lua.create_function(|_, msg: String| {
            error!("[Plugin] {}", msg);
            Ok(())
        }).map_err(|e| anyhow::anyhow!("Failed to create log.error: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set log.error: {}", e))?;

    // log.debug(message)
    log_table.set(
        "debug",
        lua.create_function(|_, msg: String| {
            debug!("[Plugin] {}", msg);
            Ok(())
        }).map_err(|e| anyhow::anyhow!("Failed to create log.debug: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set log.debug: {}", e))?;

    lua.globals().set("log", log_table)
        .map_err(|e| anyhow::anyhow!("Failed to set log global: {}", e))?;

    Ok(())
}
