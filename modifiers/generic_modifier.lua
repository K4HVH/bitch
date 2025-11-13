-- Generic Modifier Example
-- Demonstrates modifying different MAVLink message types
-- This works for ANY message type without needing Rust code changes!

function modify(ctx)
    local msg = ctx.message

    log.info(string.format("Processing %s message from system %d component %d",
        ctx.message_type, ctx.system_id, ctx.component_id))

    -- Messages use mavlink internally-tagged format: {type = "MESSAGE_TYPE", field1 = ..., field2 = ...}
    -- Access fields directly from the message table

    -- Example 1: Modify COMMAND_LONG
    if msg.type == "COMMAND_LONG" then
        log.info(string.format("  COMMAND_LONG: command=%s, target_system=%d",
            tostring(msg.command), msg.target_system))

        -- Modify any parameter
        -- msg.param1 = msg.param1 * 1.5
    end

    -- Example 2: Modify HEARTBEAT
    if msg.type == "HEARTBEAT" then
        log.info(string.format("  HEARTBEAT: type=%s, autopilot=%s, system_status=%s",
            tostring(msg.mavtype), tostring(msg.autopilot), tostring(msg.system_status)))

        -- Modify heartbeat fields
        -- Note: base_mode is a table with a 'bits' field
        -- if msg.base_mode and msg.base_mode.bits then
        --     msg.base_mode.bits = msg.base_mode.bits | 128  -- Set armed bit
        -- end
    end

    -- Example 3: Modify GLOBAL_POSITION_INT
    if msg.type == "GLOBAL_POSITION_INT" then
        log.info(string.format("  GLOBAL_POSITION_INT: lat=%d, lon=%d, alt=%d",
            msg.lat, msg.lon, msg.alt))

        -- Example: Clamp altitude
        -- if msg.alt > 100000 then
        --     msg.alt = 100000
        -- end
    end

    -- Example 4: Modify MISSION_ITEM_INT
    if msg.type == "MISSION_ITEM_INT" then
        log.info(string.format("  MISSION_ITEM_INT #%d: command=%s, x=%d, y=%d, z=%.2f",
            msg.seq, tostring(msg.command), msg.x, msg.y, msg.z))

        -- Modify mission waypoint altitude or position
        -- msg.z = msg.z * 0.8  -- Reduce altitude by 20%
    end

    -- Add more message types as needed...
    -- The generic structure means you can modify ANY MAVLink message!

    return ctx
end
