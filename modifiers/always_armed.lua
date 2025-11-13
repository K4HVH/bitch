-- Always Armed Modifier
-- Modifies HEARTBEAT messages to show drones as always armed
-- Sets the MAV_MODE_FLAG_SAFETY_ARMED bit (128) in base_mode

function modify(ctx)
    local msg = ctx.message

    -- Messages are serialized as {MESSAGE_TYPE = {fields...}}
    if msg.HEARTBEAT then
        local hb = msg.HEARTBEAT

        -- base_mode is a table/enum with a 'bits' field containing the actual value
        if hb.base_mode and hb.base_mode.bits then
            local original_mode = hb.base_mode.bits

            -- MAV_MODE_FLAG_SAFETY_ARMED = 128 (0x80)
            -- Set bit 7 to indicate armed
            local armed_bit = 128
            local new_mode = original_mode | armed_bit

            -- Only log if we're actually changing the value
            if original_mode ~= new_mode then
                log.info(string.format("Setting armed bit on sys=%d: base_mode %d -> %d (0x%02X -> 0x%02X)",
                    ctx.system_id, original_mode, new_mode, original_mode, new_mode))
            end

            hb.base_mode.bits = new_mode
            msg.HEARTBEAT = hb
            ctx.message = msg
        else
            log.error("Could not access base_mode.bits field in HEARTBEAT message")
        end
    else
        log.error(string.format("Expected HEARTBEAT, got message_type: %s", ctx.message_type))
    end

    return ctx
end
