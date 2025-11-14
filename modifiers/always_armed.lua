-- Always Armed Modifier
-- Modifies HEARTBEAT messages to show drones as always armed
-- Sets the MAV_MODE_FLAG_SAFETY_ARMED bit (128) in base_mode
-- If activated by trigger, only affects the drone that was targeted by the triggering command

function modify(ctx)
    local msg = ctx.message

    -- Check if activated by trigger with context
    if ctx.trigger_context and ctx.trigger_context.message then
        local trigger_msg = ctx.trigger_context.message

        -- Extract target_system from triggering command (e.g., ARM command)
        if trigger_msg.target_system then
            local target_system = trigger_msg.target_system
            -- Only modify HEARTBEATs from the drone that was targeted
            if ctx.system_id ~= target_system then
                -- This HEARTBEAT is not from the target drone, skip modification
                return ctx
            end
        end
    end

    -- Messages use mavlink internally-tagged format: {type = "MESSAGE_TYPE", field1 = ..., field2 = ...}
    if msg.type == "HEARTBEAT" then
        -- base_mode is a table/enum with a 'bits' field containing the actual value
        if msg.base_mode and msg.base_mode.bits then
            local original_mode = msg.base_mode.bits

            -- MAV_MODE_FLAG_SAFETY_ARMED = 128 (0x80)
            -- Set bit 7 to indicate armed
            local armed_bit = 128
            local new_mode = original_mode | armed_bit

            -- Only log if we're actually changing the value
            if original_mode ~= new_mode then
                log.info(string.format("Setting armed bit on sys=%d: base_mode %d -> %d (0x%02X -> 0x%02X)",
                    ctx.system_id, original_mode, new_mode, original_mode, new_mode))
            end

            msg.base_mode.bits = new_mode
        else
            log.error("Could not access base_mode.bits field in HEARTBEAT message")
        end
    else
        log.error(string.format("Expected HEARTBEAT, got message_type: %s", ctx.message_type))
    end

    return ctx
end
