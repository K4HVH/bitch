-- ARM Notification Plugin
-- Sends serial notification to Arduino when a drone is about to ARM

function format_droneid(target_system, base)
    -- Calculate drone ID: (target_system - base) % 10
    local adjusted = target_system - base
    if adjusted < 0 then
        adjusted = 0
    end

    local drone_id = adjusted % 10
    if drone_id == 0 then
        drone_id = 10
    end

    -- Format as d01d, d02d, ..., d10d
    if drone_id == 10 then
        return "d10d"
    else
        return string.format("d0%dd", drone_id)
    end
end

function on_match(ctx)
    -- For COMMAND_LONG messages, the fields are directly in ctx.message
    -- (not nested under ctx.message.COMMAND_LONG)
    local msg = ctx.message

    -- Verify this is a COMMAND_LONG message
    if ctx.message_type ~= "COMMAND_LONG" then
        log.error(string.format("Expected COMMAND_LONG message, got message_type: %s", ctx.message_type))
        return
    end

    -- Get target_system directly from message
    local target_sys = msg.target_system

    if not target_sys then
        log.error("target_system field not found in COMMAND_LONG message")
        return
    end

    log.info(string.format("ARM detected for system %d", target_sys))

    -- Calculate drone ID message
    local drone_msg = format_droneid(target_sys, 100)

    log.info(string.format("Sending to Arduino: %s", drone_msg))

    -- Send to Arduino via serial
    local success = serial.write("/dev/ttyUSB0", 57600, drone_msg, 3000)

    if success then
        log.info("Serial notification sent successfully")
    else
        log.error("Failed to send serial notification")
    end
end
