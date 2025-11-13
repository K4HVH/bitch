-- Generic Modifier Example
-- Demonstrates modifying different MAVLink message types
-- This works for ANY message type without needing Rust code changes!

function modify(ctx)
    local msg = ctx.message

    log.info(string.format("Processing %s message from system %d component %d",
        ctx.message_type, ctx.system_id, ctx.component_id))

    -- Messages are serialized as {MESSAGE_TYPE = {fields...}}
    -- Access fields from the nested structure

    -- Example 1: Modify COMMAND_LONG
    if msg.COMMAND_LONG then
        local cmd = msg.COMMAND_LONG
        log.info(string.format("  COMMAND_LONG: command=%s, target_system=%d",
            tostring(cmd.command), cmd.target_system))

        -- Modify any parameter
        -- cmd.param1 = cmd.param1 * 1.5

        msg.COMMAND_LONG = cmd
        ctx.message = msg
    end

    -- Example 2: Modify HEARTBEAT
    if msg.HEARTBEAT then
        local hb = msg.HEARTBEAT
        log.info(string.format("  HEARTBEAT: type=%s, autopilot=%s, system_status=%s",
            tostring(hb.mavtype), tostring(hb.autopilot), tostring(hb.system_status)))

        -- Modify heartbeat fields
        -- Note: base_mode is a table with a 'bits' field
        -- if hb.base_mode and hb.base_mode.bits then
        --     hb.base_mode.bits = hb.base_mode.bits | 128  -- Set armed bit
        -- end

        msg.HEARTBEAT = hb
        ctx.message = msg
    end

    -- Example 3: Modify GLOBAL_POSITION_INT
    if msg.GLOBAL_POSITION_INT then
        local pos = msg.GLOBAL_POSITION_INT
        log.info(string.format("  GLOBAL_POSITION_INT: lat=%d, lon=%d, alt=%d",
            pos.lat, pos.lon, pos.alt))

        -- Example: Clamp altitude
        -- if pos.alt > 100000 then
        --     pos.alt = 100000
        -- end

        msg.GLOBAL_POSITION_INT = pos
        ctx.message = msg
    end

    -- Example 4: Modify MISSION_ITEM_INT
    if msg.MISSION_ITEM_INT then
        local item = msg.MISSION_ITEM_INT
        log.info(string.format("  MISSION_ITEM_INT #%d: command=%s, x=%d, y=%d, z=%.2f",
            item.seq, tostring(item.command), item.x, item.y, item.z))

        -- Modify mission waypoint altitude or position
        -- item.z = item.z * 0.8  -- Reduce altitude by 20%

        msg.MISSION_ITEM_INT = item
        ctx.message = msg
    end

    -- Add more message types as needed...
    -- The generic structure means you can modify ANY MAVLink message!

    return ctx
end
