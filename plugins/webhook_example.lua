-- Webhook Example Plugin
-- Demonstrates HTTP POST to a webhook when an ARM command is detected

function on_match(ctx)
    log.info("Sending webhook notification")

    local msg = ctx.message

    -- Messages use mavlink internally-tagged format: {type = "MESSAGE_TYPE", field1 = ..., field2 = ...}
    -- Access fields directly from the message table

    -- Build JSON payload
    local payload = string.format([[{
        "event": "arm_command",
        "system_id": %d,
        "component_id": %d,
        "message_type": "%s",
        "target_system": %d,
        "target_component": %d,
        "command": "%s",
        "timestamp": %d
    }]],
        ctx.system_id,
        ctx.component_id,
        ctx.message_type,
        msg.target_system or 0,
        msg.target_component or 0,
        tostring(msg.command or "unknown"),
        os.time()
    )

    -- Send to webhook (example URL - replace with your own)
    -- local response = http.post("https://example.com/webhook", payload)

    -- if response then
    --     log.info("Webhook sent successfully")
    -- else
    --     log.warn("Webhook failed")
    -- end

    log.debug("Webhook plugin executed (webhook URL commented out)")
end
