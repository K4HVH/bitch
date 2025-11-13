# BITCH - Complete Documentation
**B**asic **I**ntercept & **T**ransform **C**ommand **H**andler

A powerful MAVLink proxy that intercepts and transforms messages between Ground Control Station (GCS) and drones via mavlink-router.

---

## Table of Contents

1. [Architecture](#architecture)
2. [Features](#features)
3. [Installation & Usage](#installation--usage)
4. [Configuration System](#configuration-system)
5. [Rules System](#rules-system)
6. [Modifier System](#modifier-system)
7. [Plugin System](#plugin-system)
8. [Advanced Examples](#advanced-examples)
9. [Technical Details](#technical-details)
10. [Development](#development)
11. [Troubleshooting](#troubleshooting)

---

## Architecture

```
Mission Planner (GCS) <--> BITCH <--> mavlink-router <--> Drones
                      :14550            :14551
```

BITCH acts as a transparent bidirectional UDP proxy, parsing MAVLink messages and applying rules to:
- **GCS → Router**: Full rule processing with all actions
- **Router → GCS**: Full rule processing with all actions

Both directions support all actions: forward, block, modify, delay, and batch.

---

## Features

- **Transparent bidirectional UDP proxy** between GCS and mavlink-router
- **MAVLink message parsing** (supports both v1 and v2)
- **Rule-based filtering system** with conditions, priorities, and direction control
- **Flexible actions**: delay, block, forward, modify, batch
- **Generic message support** - Works with ALL 300+ MAVLink message types
- **Conditional matching** on ANY message field
- **Priority-based rule processing** for complex logic
- **Direction control**: Apply rules to GCS→Router, Router→GCS, or both
- **Lua scripting** for modifiers and plugins (unified API)
- **Batch synchronization** across multiple drones with configurable field extraction
- **Generic Auto-ACK** for ANY message type (not just COMMAND_LONG)
- **Async/non-blocking operation** using Tokio
- **Detailed logging** with tracing

---

## Installation & Usage

### Build
```bash
cargo build --release
```

Binary will be at `target/release/bitch`

### Run
```bash
cargo run --release
```

Or directly:
```bash
./target/release/bitch
```

### Configure Mission Planner
1. Go to connection settings
2. Set UDP connection to: `127.0.0.1:14550` (or your machine's IP if remote)
3. Connect

### Prerequisites
- mavlink-router running on port 14551 (configured at `/etc/mavlink-router/main.conf`)
- Rust toolchain for building

---

## Configuration System

Configuration is managed via `config.toml` with the following sections:

### Network Configuration
```toml
[network]
gcs_listen_port = 14550        # Port for GCS connections
gcs_listen_address = "0.0.0.0" # Listen on all interfaces
router_address = "127.0.0.1"   # mavlink-router address
router_port = 14551            # mavlink-router port
```

### Logging Configuration
```toml
[logging]
level = "info"  # Options: trace, debug, info, warn, error
```

- `trace`: Very verbose, shows all packet details
- `debug`: Shows parsed messages and forwarding
- `info`: Shows connections and rule matches (recommended)
- `warn`: Shows warnings and blocked messages
- `error`: Only errors

### Plugin Configuration
```toml
[plugins]
directory = "plugins"

[plugins.load]
arm_notifier = "arm_notifier.lua"
webhook_example = "webhook_example.lua"
```

### Modifier Configuration
```toml
[modifiers]
directory = "modifiers"

[modifiers.load]
always_armed = "always_armed.lua"
generic_modifier = "generic_modifier.lua"
```

---

## Rules System

Rules are the core of BITCH, allowing you to intercept, modify, delay, or block MAVLink messages based on conditions.

### Rule Structure

```toml
[[rules]]
message_type = "MESSAGE_TYPE"    # Which message type to match (required)
priority = 10                    # Higher = checked first (default: 0)
actions = ["action1", "action2"] # Sequential actions to apply (required)
direction = "gcs_to_router"      # Flow direction (default: "gcs_to_router")
description = "What this rule does"

[rules.conditions]               # Match specific message fields (optional)
command = "MAV_CMD_..."          # Match command field (for messages with command enum)
param1 = 1.0                     # Match param1 (for COMMAND_LONG)
system_id = 1                    # Match header system ID
fix_type = 3                     # Match fix_type (for GPS_RAW_INT)
# ... any field in ANY message type
```

**Key Changes from Previous Versions:**
- ❌ No more `command` field at rule level (was COMMAND_LONG-specific)
- ✅ Use `conditions.command` instead (works for ALL messages with command field)
- ❌ No more explicit `param1-7` fields in struct
- ✅ All fields go in `conditions` as generic key-value pairs

### Direction Control

Rules can apply to specific message flows:

- `"gcs_to_router"` - Messages from GCS to Router (default)
- `"router_to_gcs"` - Messages from Router to GCS (drone telemetry)
- `"both"` - Messages in both directions

ALL actions (forward, block, modify, delay, batch) work in BOTH directions.

### Available Actions

#### 1. Forward
Immediately forward the message to destination.

```toml
actions = ["forward"]
```

#### 2. Block
Drop the message completely (never forward).

```toml
actions = ["block"]
```

#### 3. Modify
Transform message using a Lua modifier before forwarding.

```toml
actions = ["modify", "forward"]
modifier = "always_armed"
```

#### 4. Delay
Hold message for specified time before forwarding.

```toml
actions = ["delay"]
delay_seconds = 10
```

#### 5. Batch
Synchronize messages across multiple drones.

```toml
actions = ["batch", "delay"]
batch_count = 3                     # Wait for 3 unique drones
batch_timeout_seconds = 60          # Timeout after 60s
batch_timeout_forward = true        # Forward on timeout (or drop if false)
batch_key = "arm_swarm"            # Batch group identifier
batch_system_id_field = "target_system"  # NEW: Extract system_id from this field
```

**Batch System ID Extraction (NEW):**
- `batch_system_id_field` - Optional field name to extract system_id from message
- If not specified, uses `header.system_id` (sender)
- If specified, extracts from message field (e.g., `"target_system"` for COMMAND_LONG)
- Works for ANY message type!

**Example - Batch by target (recipient):**
```toml
batch_system_id_field = "target_system"  # Batch by who receives the command
```

**Example - Batch by sender (default):**
```toml
# No batch_system_id_field = uses header.system_id (who sent the message)
```

### Auto-ACK Feature (COMPLETELY GENERIC)

Automatically send ACK responses for ANY message type (not just COMMAND_LONG):

```toml
auto_ack = true
ack_message_type = "COMMAND_ACK"              # Which message type to send as ACK
ack_source_system_field = "target_system"     # Field to use as ACK source system_id
ack_source_component_field = "target_component"  # Field to use as ACK source component_id

[rules.ack_fields]
result = "MAV_RESULT_ACCEPTED"   # Fields to set in ACK message
# ... any fields for the ACK message type
```

**How it works:**
1. When rule matches, extracts `source_system` and `source_component` from the matched message
2. Builds an ACK message of type `ack_message_type` with fields from `ack_fields`
3. Sends ACK immediately (before delays/batches) so GCS doesn't timeout
4. ACK appears to come FROM the target system (not the proxy)

**Example - COMMAND_ACK for COMMAND_LONG:**
```toml
[[rules]]
message_type = "COMMAND_LONG"
auto_ack = true
ack_message_type = "COMMAND_ACK"
ack_source_system_field = "target_system"
ack_source_component_field = "target_component"

[rules.conditions]
command = "MAV_CMD_COMPONENT_ARM_DISARM"

[rules.ack_fields]
result = "MAV_RESULT_ACCEPTED"
```

**Example - MISSION_ACK for MISSION_REQUEST_LIST:**
```toml
[[rules]]
message_type = "MISSION_REQUEST_LIST"
auto_ack = true
ack_message_type = "MISSION_COUNT"
ack_source_system_field = "target_system"
ack_source_component_field = "target_component"

[rules.ack_fields]
count = 0
mission_type = "MAV_MISSION_TYPE_MISSION"
```

### Message Types

BITCH supports ALL 300+ MAVLink message types through generic handling:

**Common types:**
- `COMMAND_LONG` - Command messages
- `COMMAND_INT` - Command messages (integer coordinates)
- `HEARTBEAT` - Heartbeat messages
- `GPS_RAW_INT` - GPS data
- `GLOBAL_POSITION_INT` - Global position
- `ATTITUDE` - Attitude data
- `RC_CHANNELS` - RC input
- `MISSION_ITEM` - Mission waypoints
- `MISSION_ITEM_INT` - Mission waypoints (integer)
- `MISSION_REQUEST_LIST` - Request mission list
- `MISSION_ACK` - Mission acknowledgment
- `STATUSTEXT` - Status text messages
- `SYS_STATUS` - System status
- `BATTERY_STATUS` - Battery info
- `PARAM_REQUEST_READ` - Parameter read request
- `PARAM_VALUE` - Parameter value
- And 290+ more...

### Common Commands (for messages with command field)

**COMMAND_LONG / COMMAND_INT commands:**

| ID  | Command | Description |
|-----|---------|-------------|
| 400 | MAV_CMD_COMPONENT_ARM_DISARM | Arm/disarm motors |
| 300 | MAV_CMD_MISSION_START | Start mission |
| 176 | MAV_CMD_DO_SET_MODE | Change flight mode |
| 22  | MAV_CMD_NAV_TAKEOFF | Takeoff command |
| 21  | MAV_CMD_NAV_LAND | Land command |
| 16  | MAV_CMD_NAV_WAYPOINT | Waypoint navigation |
| 183 | MAV_CMD_DO_SET_SERVO | Set servo position |
| 181 | MAV_CMD_DO_SET_RELAY | Set relay state |

**MISSION_ITEM / MISSION_ITEM_INT commands:**

| ID  | Command | Description |
|-----|---------|-------------|
| 16  | MAV_CMD_NAV_WAYPOINT | Waypoint |
| 22  | MAV_CMD_NAV_TAKEOFF | Takeoff waypoint |
| 21  | MAV_CMD_NAV_LAND | Land waypoint |
| 19  | MAV_CMD_NAV_LOITER_UNLIM | Loiter unlimited |
| 177 | MAV_CMD_DO_JUMP | Jump to waypoint |

### Conditions

Conditions allow matching on specific fields within messages. ALL fields work the same way - no special cases!

#### Header Conditions (work for ALL message types)

```toml
[rules.conditions]
system_id = 1        # Match messages from system ID 1
component_id = 1     # Match messages from component ID 1
```

#### Message Field Conditions (COMPLETELY GENERIC)

Match ANY field in ANY message type:

```toml
[rules.conditions]
# COMMAND_LONG fields
command = "MAV_CMD_COMPONENT_ARM_DISARM"
param1 = 1.0
param2 = 0.0
target_system = 1
target_component = 1

# GPS_RAW_INT fields
fix_type = 3
satellites_visible = 10

# HEARTBEAT fields
system_status = "MAV_STATE_ACTIVE"
autopilot = "MAV_AUTOPILOT_ARDUPILOTMEGA"

# GLOBAL_POSITION_INT fields
alt = 1000

# RC_CHANNELS fields
chan1_raw = 1500

# ANY field in ANY message type - completely generic!
```

**Field matching supports:**
- **Integers**: Exact match
- **Floats**: Within epsilon
- **Strings**: Contains match OR exact match
- **Booleans**: Exact match
- **Enums**: String contains match (e.g., `"MAV_CMD_..."`)

---

## Modifier System

Modifiers are Lua scripts that transform MAVLink messages before forwarding. They work with ALL 300+ message types!

### Modifier Structure

Modifiers must implement a `modify()` function:

```lua
function modify(ctx)
    local msg = ctx.message

    -- Messages are serialized as {MESSAGE_TYPE = {fields...}}
    if msg.COMMAND_LONG then
        local cmd = msg.COMMAND_LONG
        -- Modify fields
        cmd.param1 = cmd.param1 * 2
        msg.COMMAND_LONG = cmd
        ctx.message = msg
    end

    return ctx
end
```

### Context Structure

```lua
ctx = {
    system_id = 1,              -- Header system ID
    component_id = 1,           -- Header component ID
    sequence = 123,             -- Header sequence number
    message_type = "COMMAND_LONG",  -- Message type name
    message = {                 -- Message data (nested structure)
        COMMAND_LONG = {        -- Or HEARTBEAT, GPS_RAW_INT, etc.
            command = ...,
            param1 = ...,
            target_system = ...,
            -- ... all fields
        }
    }
}
```

### Available APIs

```lua
log.info(message)
log.warn(message)
log.error(message)
log.debug(message)
```

### Examples

#### Example 1: Modify COMMAND_LONG parameters

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.COMMAND_LONG then
        local cmd = msg.COMMAND_LONG

        -- Double param1 value
        cmd.param1 = cmd.param1 * 2

        log.info(string.format("Modified param1 for cmd %s", tostring(cmd.command)))

        msg.COMMAND_LONG = cmd
        ctx.message = msg
    end

    return ctx
end
```

#### Example 2: Modify HEARTBEAT to show always armed

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.HEARTBEAT then
        local hb = msg.HEARTBEAT

        -- base_mode is a table with 'bits' field for bitflags
        if hb.base_mode and hb.base_mode.bits then
            local armed_bit = 128  -- MAV_MODE_FLAG_SAFETY_ARMED
            hb.base_mode.bits = hb.base_mode.bits | armed_bit

            msg.HEARTBEAT = hb
            ctx.message = msg
        end
    end

    return ctx
end
```

#### Example 3: Clamp altitude in GLOBAL_POSITION_INT

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.GLOBAL_POSITION_INT then
        local pos = msg.GLOBAL_POSITION_INT

        -- Limit altitude to 100m
        if pos.alt > 100000 then  -- Altitude in millimeters
            pos.alt = 100000
            log.warn("Clamped altitude to 100m")
        end

        msg.GLOBAL_POSITION_INT = pos
        ctx.message = msg
    end

    return ctx
end
```

#### Example 4: Modify MISSION_ITEM_INT waypoint

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.MISSION_ITEM_INT then
        local item = msg.MISSION_ITEM_INT

        -- Reduce all waypoint altitudes by 20%
        item.z = item.z * 0.8

        log.info(string.format("Modified waypoint #%d altitude", item.seq))

        msg.MISSION_ITEM_INT = item
        ctx.message = msg
    end

    return ctx
end
```

### Enum/Bitflag Handling

Some fields are enums or bitflags and serialize as tables:

```lua
-- Bitflags (base_mode, custom_mode, etc.)
if hb.base_mode.bits then
    local mode = hb.base_mode.bits
    hb.base_mode.bits = mode | 128  -- Set bit
end

-- Enums (typically accessed as values)
local cmd = msg.COMMAND_LONG.command  -- Already the enum value
```

---

## Plugin System

Plugins are Lua scripts that execute side effects when rules match. They have the same API as modifiers but DON'T return modified messages.

### Plugin Structure

Plugins must implement an `on_match()` function:

```lua
function on_match(ctx)
    local msg = ctx.message

    -- Messages are serialized as {MESSAGE_TYPE = {fields...}}
    if msg.COMMAND_LONG then
        local cmd = msg.COMMAND_LONG

        -- Do something (send notification, log, etc.)
        log.info(string.format("ARM command for system %d", cmd.target_system))
    end
end
```

### Context Structure

**Identical to modifiers:**

```lua
ctx = {
    system_id = 1,              -- Header system ID
    component_id = 1,           -- Header component ID
    message_type = "COMMAND_LONG",  -- Message type name
    message = {                 -- Message data (nested structure)
        COMMAND_LONG = {        -- Or HEARTBEAT, GPS_RAW_INT, etc.
            command = ...,
            param1 = ...,
            target_system = ...,
            -- ... all fields
        }
    }
}
```

### Available APIs

**Logging:**
```lua
log.info(message)
log.warn(message)
log.error(message)
log.debug(message)
```

**Serial Communication:**
```lua
success = serial.write(port, baudrate, data, timeout_ms)
success = serial.write_line(port, baudrate, data, timeout_ms)
```

**HTTP Requests:**
```lua
response = http.get(url, headers_table)
response = http.post(url, body, headers_table)
```

**Utilities:**
```lua
util.sleep(milliseconds)
success = util.file_write(path, content)
content = util.file_read(path)
```

### Examples

#### Example 1: Send serial notification on ARM

```lua
function on_match(ctx)
    local msg = ctx.message

    if msg.COMMAND_LONG then
        local cmd = msg.COMMAND_LONG

        -- Calculate drone ID
        local drone_id = (cmd.target_system - 100) % 10
        local message = string.format("d%02dd", drone_id)

        -- Send to serial device
        local success = serial.write("/dev/ttyUSB0", 57600, message, 3000)

        if success then
            log.info("Serial notification sent")
        else
            log.error("Failed to send serial notification")
        end
    end
end
```

#### Example 2: Send webhook on ARM command

```lua
function on_match(ctx)
    local msg = ctx.message

    if msg.COMMAND_LONG then
        local cmd = msg.COMMAND_LONG

        -- Build JSON payload
        local payload = string.format([[{
            "event": "arm_command",
            "system_id": %d,
            "target_system": %d,
            "command": "%s",
            "timestamp": %d
        }]], ctx.system_id, cmd.target_system,
            tostring(cmd.command), os.time())

        -- Send webhook
        local response = http.post("https://example.com/webhook", payload)

        if response then
            log.info("Webhook sent successfully")
        else
            log.warn("Webhook failed")
        end
    end
end
```

#### Example 3: Log GPS fix status

```lua
function on_match(ctx)
    local msg = ctx.message

    if msg.GPS_RAW_INT then
        local gps = msg.GPS_RAW_INT

        log.info(string.format("GPS Fix: type=%d, sats=%d, lat=%d, lon=%d",
            gps.fix_type, gps.satellites_visible, gps.lat, gps.lon))
    end
end
```

---

## Advanced Examples

### Example 1: Synchronize ARM Across Swarm

Batch ARM commands from multiple drones, delay, then forward:

```toml
[[rules]]
message_type = "COMMAND_LONG"
actions = ["batch", "delay"]
batch_count = 3                          # Wait for 3 drones
batch_timeout_seconds = 30
batch_timeout_forward = true
batch_key = "arm_swarm"
batch_system_id_field = "target_system"  # Batch by target (recipient)
delay_seconds = 5
auto_ack = true
ack_message_type = "COMMAND_ACK"
ack_source_system_field = "target_system"
ack_source_component_field = "target_component"
plugins = ["arm_notifier"]
direction = "gcs_to_router"
description = "Synchronize ARM across 3 drones with 5s delay"

[rules.conditions]
command = "MAV_CMD_COMPONENT_ARM_DISARM"
param1 = 1.0  # Only ARM (not DISARM)

[rules.ack_fields]
result = "MAV_RESULT_ACCEPTED"
```

### Example 2: Block Emergency LAND Commands

Prevent emergency land from specific system:

```toml
[[rules]]
message_type = "COMMAND_LONG"
actions = ["block"]
priority = 100  # High priority
direction = "gcs_to_router"
description = "Block emergency LAND from system 255"

[rules.conditions]
command = "MAV_CMD_NAV_LAND"
param1 = 1.0  # Emergency flag
system_id = 255  # GCS
```

### Example 3: Modify HEARTBEAT from Drones

Make all drones appear armed:

```toml
[[rules]]
message_type = "HEARTBEAT"
actions = ["modify", "forward"]
modifier = "always_armed"
direction = "router_to_gcs"
description = "Show drones as always armed"
```

### Example 4: Delay GPS Data with 3D Fix

Add 1 second delay to GPS messages:

```toml
[[rules]]
message_type = "GPS_RAW_INT"
actions = ["delay"]
delay_seconds = 1
direction = "router_to_gcs"
description = "Delay GPS with 3D fix"

[rules.conditions]
fix_type = 3  # GPS_FIX_TYPE_3D_FIX
```

### Example 5: Block Error Messages

Filter out error STATUSTEXT messages:

```toml
[[rules]]
message_type = "STATUSTEXT"
actions = ["block"]
direction = "router_to_gcs"
description = "Block error status messages"

[rules.conditions]
text = "error"  # Contains "error"
```

### Example 6: Batch Mission Items Across Fleet

Synchronize mission uploads:

```toml
[[rules]]
message_type = "MISSION_ITEM_INT"
actions = ["batch"]
batch_count = 5                          # Wait for 5 drones
batch_timeout_seconds = 10
batch_key = "mission_sync"
batch_system_id_field = "target_system"
direction = "gcs_to_router"
description = "Sync mission items across fleet"
```

### Example 7: Auto-ACK Mission Requests

Immediately acknowledge mission list requests:

```toml
[[rules]]
message_type = "MISSION_REQUEST_LIST"
actions = ["delay"]
delay_seconds = 2
auto_ack = true
ack_message_type = "MISSION_COUNT"
ack_source_system_field = "target_system"
ack_source_component_field = "target_component"
direction = "gcs_to_router"
description = "ACK mission requests"

[rules.ack_fields]
count = 0
mission_type = "MAV_MISSION_TYPE_MISSION"
```

---

## Technical Details

### Message Flow

```
1. UDP Packet Received
2. Parse MAVLink (v2/v1)
3. Extract message type
4. Find matching rule (priority order)
   - Check direction
   - Check message_type
   - Check conditions (ALL fields generic)
5. Execute plugins (if any)
6. Build action sequence
7. Execute modifiers (if modify action)
8. Send ACK (if auto_ack)
9. Execute actions recursively:
   - Forward → send packet
   - Block → drop packet
   - Modify → reconstruct packet
   - Delay → spawn async task
   - Batch → queue or release
```

### Rule Processing

- Rules are sorted by **priority** (highest first) at startup
- First matching rule wins
- If no rule matches, message is forwarded immediately
- Direction filter applied first (efficient)
- Message type check (exact string match)
- Conditions checked last (generic field matching via JSON)

### Action Execution

Actions execute **sequentially** in the order specified:

```toml
actions = ["modify", "batch", "delay", "forward"]
```

1. **modify** - Transform message
2. **batch** - Queue until threshold met
3. **delay** - Wait N seconds
4. **forward** - Send to destination

### Batch Behavior

**Threshold Met:**
- All queued packets released
- Remaining actions applied to ALL packets
- Batch removed from memory

**Timeout:**
- If `batch_timeout_forward = true`: Forward all packets directly (no remaining actions)
- If `batch_timeout_forward = false`: Drop all packets
- Warning logged with statistics

### Auto-ACK Behavior

**When enabled:**
1. Extract source system/component from matched message using configured fields
2. Build ACK message of specified type with configured fields
3. Send ACK **immediately** (before delays/batches)
4. ACK appears to come FROM target system (not proxy)
5. GCS receives instant ACK and doesn't wait for delayed command

**ACK Source:**
```rust
MavHeader {
    system_id: extracted_from_message,  // From ack_source_system_field
    component_id: extracted_from_message,  // From ack_source_component_field
    sequence: 0,
}
```

### Generic Field Extraction

**All systems use the same generic extraction:**

```rust
// Serialize message to JSON
let msg_json = serde_json::to_value(msg)?;

// Get message type wrapper
let msg_data = msg_json.get("MESSAGE_TYPE")?;

// Extract any field
let field_value = msg_data.get("field_name")?;
```

**Used for:**
- Condition matching (ANY field in ANY message)
- Auto-ACK field extraction (source_system, source_component)
- Batch system_id extraction (configurable field)

**No special cases for any message type!**

---

## Development

### Adding New Rules

1. Edit `config.toml`
2. Add `[[rules]]` section
3. Specify `message_type` and `actions`
4. Add `[rules.conditions]` if needed
5. Reload BITCH (it reads config on startup)

### Creating Modifiers

1. Create `modifiers/my_modifier.lua`
2. Implement `modify(ctx)` function
3. Add to `config.toml`:
   ```toml
   [modifiers.load]
   my_modifier = "my_modifier.lua"
   ```
4. Reference in rule:
   ```toml
   modifier = "my_modifier"
   ```

### Creating Plugins

1. Create `plugins/my_plugin.lua`
2. Implement `on_match(ctx)` function
3. Add to `config.toml`:
   ```toml
   [plugins.load]
   my_plugin = "my_plugin.lua"
   ```
4. Reference in rule:
   ```toml
   plugins = ["my_plugin"]
   ```

### Testing

```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Run with trace logging (very verbose)
RUST_LOG=trace cargo run
```

### Building for Production

```bash
cargo build --release
strip target/release/bitch
```

---

## Troubleshooting

### Messages Not Being Intercepted

- Check `direction` field matches message flow
- Verify `message_type` is correct (check logs with `RUST_LOG=debug`)
- Ensure conditions match (check field values in logs)
- Check rule priority (higher priority rules checked first)

### Lua Script Errors

- Check logs for Lua error messages
- Verify script syntax: `lua -c script.lua`
- Ensure message structure access is correct (nested under MESSAGE_TYPE)
- Check that you're returning `ctx` from `modify()` functions

### Auto-ACK Not Working

- Verify `auto_ack = true`
- Check `ack_message_type` is correct
- Verify `ack_source_system_field` and `ack_source_component_field` exist in message
- Check `[rules.ack_fields]` section is complete
- Look for "Failed to build ACK" warnings in logs

### Batch Not Releasing

- Check `batch_count` threshold
- Verify `batch_system_id_field` extracts correct field
- Check timeout duration
- Look for "Batch timeout" warnings
- Ensure unique system IDs are being tracked (not total packet count)

### Performance Issues

- Reduce logging level (use `info` or `warn`)
- Avoid expensive operations in plugins
- Check for blocking operations (use async APIs)
- Monitor with `RUST_LOG=info`

### Connection Issues

- Verify mavlink-router is running on port 14551
- Check firewall rules
- Ensure no port conflicts
- Test with `nc -u -l 14550` to verify GCS can connect

---

## License

This project is for educational and research purposes.

## Contributing

Contributions welcome! Please ensure:
- Code follows Rust best practices
- Tests pass (`cargo test`)
- Logging is appropriate
- Documentation is updated

## Support

For issues or questions, check logs with `RUST_LOG=debug` first, then open an issue with:
- Log output
- Config file
- Expected vs actual behavior
