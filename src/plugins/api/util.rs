use anyhow::Result;
use mlua::Lua;

/// Initialize utility API for Lua
pub fn init(lua: &Lua) -> Result<()> {
    let util_table = lua.create_table()
        .map_err(|e| anyhow::anyhow!("Failed to create util table: {}", e))?;

    // util.sleep(milliseconds)
    util_table.set(
        "sleep",
        lua.create_async_function(|_, ms: u64| async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
            Ok(())
        }).map_err(|e| anyhow::anyhow!("Failed to create util.sleep: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set util.sleep: {}", e))?;

    // util.file_write(path, content)
    util_table.set(
        "file_write",
        lua.create_function(|_, (path, content): (String, String)| {
            match std::fs::write(&path, content) {
                Ok(_) => Ok(true),
                Err(e) => {
                    tracing::warn!("[Plugin] Failed to write file {}: {}", path, e);
                    Ok(false)
                }
            }
        }).map_err(|e| anyhow::anyhow!("Failed to create util.file_write: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set util.file_write: {}", e))?;

    // util.file_read(path)
    util_table.set(
        "file_read",
        lua.create_function(|lua, path: String| {
            match std::fs::read_to_string(&path) {
                Ok(content) => lua.create_string(&content)
                    .map(mlua::Value::String)
                    .map_err(|e| mlua::Error::external(e)),
                Err(e) => {
                    tracing::warn!("[Plugin] Failed to read file {}: {}", path, e);
                    Ok(mlua::Value::Nil)
                }
            }
        }).map_err(|e| anyhow::anyhow!("Failed to create util.file_read: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set util.file_read: {}", e))?;

    lua.globals().set("util", util_table)
        .map_err(|e| anyhow::anyhow!("Failed to set util global: {}", e))?;

    Ok(())
}
