use anyhow::Result;
use mlua::Lua;
use tracing::{debug, warn};

/// Initialize HTTP API for Lua
pub fn init(lua: &Lua) -> Result<()> {
    let http_table = lua.create_table()
        .map_err(|e| anyhow::anyhow!("Failed to create http table: {}", e))?;

    // http.get(url, [headers_table])
    http_table.set(
        "get",
        lua.create_async_function(|lua, (url, _headers): (String, Option<mlua::Value>)| async move {
            match http_get(&url).await {
                Ok(body) => lua.create_string(&body)
                    .map(mlua::Value::String)
                    .map_err(mlua::Error::external),
                Err(e) => {
                    warn!("[Plugin] HTTP GET to {} failed: {}", url, e);
                    Ok(mlua::Value::Nil)
                }
            }
        }).map_err(|e| anyhow::anyhow!("Failed to create http.get: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set http.get: {}", e))?;

    // http.post(url, body, [headers_table])
    http_table.set(
        "post",
        lua.create_async_function(|lua, (url, body, _headers): (String, String, Option<mlua::Value>)| async move {
            match http_post(&url, body).await {
                Ok(response) => lua.create_string(&response)
                    .map(mlua::Value::String)
                    .map_err(mlua::Error::external),
                Err(e) => {
                    warn!("[Plugin] HTTP POST to {} failed: {}", url, e);
                    Ok(mlua::Value::Nil)
                }
            }
        }).map_err(|e| anyhow::anyhow!("Failed to create http.post: {}", e))?,
    ).map_err(|e| anyhow::anyhow!("Failed to set http.post: {}", e))?;

    lua.globals().set("http", http_table)
        .map_err(|e| anyhow::anyhow!("Failed to set http global: {}", e))?;

    Ok(())
}

async fn http_get(url: &str) -> Result<String> {
    debug!("[Plugin] HTTP GET: {}", url);

    let client = reqwest::Client::new();
    let response = client.get(url).send().await?;
    let body = response.text().await?;

    Ok(body)
}

async fn http_post(url: &str, body: String) -> Result<String> {
    debug!("[Plugin] HTTP POST: {}", url);

    let client = reqwest::Client::new();
    let response = client.post(url).body(body).send().await?;
    let text = response.text().await?;

    Ok(text)
}
