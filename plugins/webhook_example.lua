-- Webhook Example Plugin
-- Demonstrates HTTP POST to a webhook when an ARM command is detected

function on_match(ctx)
    log.info("Sending webhook notification")

    -- Build JSON payload
    local payload = string.format([[{
        "event": "arm_command",
        "target_system": %d,
        "target_component": %d,
        "message_type": "%s",
        "command": "%s",
        "timestamp": %d
    }]],
        ctx.target_system,
        ctx.target_component,
        ctx.message_type,
        ctx.command or "unknown",
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
