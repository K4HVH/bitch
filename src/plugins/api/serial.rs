use anyhow::{Context, Result};
use mlua::Lua;
use std::time::Duration;
use tracing::{debug, warn};

/// Initialize serial API for Lua
pub fn init(lua: &Lua) -> Result<()> {
    let serial_table = lua.create_table()
        .map_err(|e| anyhow::anyhow!("Failed to create serial table: {}", e))?;

    // serial.write(port, baudrate, data, [timeout_ms])
    serial_table.set(
        "write",
        lua.create_function(|_, (port, baudrate, data, timeout): (String, u32, String, Option<u64>)| {
            let timeout_ms = timeout.unwrap_or(3000);

            match write_serial(&port, baudrate, data.as_bytes(), timeout_ms) {
                Ok(_) => {
                    debug!("[Plugin] Serial write to {} succeeded", port);
                    Ok(true)
                }
                Err(e) => {
                    warn!("[Plugin] Serial write to {} failed: {}", port, e);
                    Ok(false)
                }
            }
        }).map_err(|e| anyhow::anyhow!("Failed to create serial.write: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set serial.write: {}", e))?;

    // serial.write_line(port, baudrate, data, [timeout_ms])
    serial_table.set(
        "write_line",
        lua.create_function(|_, (port, baudrate, data, timeout): (String, u32, String, Option<u64>)| {
            let timeout_ms = timeout.unwrap_or(3000);
            let mut line_data = data;
            line_data.push('\n');

            match write_serial(&port, baudrate, line_data.as_bytes(), timeout_ms) {
                Ok(_) => {
                    debug!("[Plugin] Serial write_line to {} succeeded", port);
                    Ok(true)
                }
                Err(e) => {
                    warn!("[Plugin] Serial write_line to {} failed: {}", port, e);
                    Ok(false)
                }
            }
        }).map_err(|e| anyhow::anyhow!("Failed to create serial.write_line: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set serial.write_line: {}", e))?;

    lua.globals().set("serial", serial_table)
        .map_err(|e| anyhow::anyhow!("Failed to set serial global: {}", e))?;

    Ok(())
}

fn write_serial(port: &str, baudrate: u32, data: &[u8], timeout_ms: u64) -> Result<()> {
    let mut port = serialport::new(port, baudrate)
        .timeout(Duration::from_millis(timeout_ms))
        .open()
        .with_context(|| format!("Failed to open serial port {}", port))?;

    port.write_all(data)
        .context("Failed to write to serial port")?;

    Ok(())
}
