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
- **Per-command rule configuration** via TOML file
- **Conditional matching** on any message field
- **Priority-based rule processing** for complex logic
- **Direction control**: Apply rules to GCS→Router, Router→GCS, or both
- **Lua scripting** for modifiers and plugins (works with ALL MAVLink message types)
- **Batch synchronization** across multiple drones
- **Auto-ACK** for command acknowledgment
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
message_type = "MESSAGE_TYPE"    # Which message type to match
command = "COMMAND_NAME"         # [COMMAND_LONG only] Specific command
priority = 10                    # Higher = checked first (default: 0)
actions = ["action1", "action2"] # Sequential actions to apply
direction = "gcs_to_router"      # Flow direction (see below)
description = "What this rule does"

[rules.conditions]               # Match specific message fields
param1 = 1.0                     # [COMMAND_LONG] Match param values
system_id = 1                    # Match header system ID
fix_type = 3                     # [GPS_RAW_INT] Match fix type
# ... any field in the message type
```

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
batch_count = 3                    # Wait for 3 unique drones
batch_timeout_seconds = 60         # Timeout after 60s
batch_timeout_forward = true       # Forward on timeout
batch_key = "arm_swarm"           # Batch group identifier
```

### Auto-ACK Feature

For COMMAND_LONG messages, automatically send COMMAND_ACK to GCS:

```toml
auto_ack = true  # GCS won't wait for delayed command
```

### Message Types

BITCH supports ALL 300+ MAVLink message types through generic handling:

**Common types:**
- `COMMAND_LONG` - Command messages
- `HEARTBEAT` - Heartbeat messages
- `GPS_RAW_INT` - GPS data
- `GLOBAL_POSITION_INT` - Global position
- `ATTITUDE` - Attitude data
- `RC_CHANNELS` - RC input
- `MISSION_ITEM` - Mission waypoints
- `MISSION_ITEM_INT` - Mission waypoints (integer)
- `STATUSTEXT` - Status text messages
- `SYS_STATUS` - System status
- `BATTERY_STATUS` - Battery info
- And 290+ more...

### Common Commands (for COMMAND_LONG)

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

### Conditions

Match on any field in the message data:

**For COMMAND_LONG:**
```toml
[rules.conditions]
param1 = 1.0              # Match param1 value
param2 = 0.0              # Match param2 value
system_id = 1             # Match system ID
component_id = 1          # Match component ID
```

**For other messages:**
```toml
[rules.conditions]
fix_type = 3              # [GPS_RAW_INT] Match GPS fix type
lat = 473566094           # [GLOBAL_POSITION_INT] Match latitude
base_mode = 89            # [HEARTBEAT] Match base mode
```

Use MAVLink documentation to find field names for each message type.

### Priority System

Rules are evaluated in priority order (highest first):

```toml
[[rules]]
priority = 100  # Checked first
# ...

[[rules]]
priority = 50   # Checked second
# ...

[[rules]]
priority = 0    # Checked last (default)
# ...
```

Use priorities to create exception rules that override general rules.

### Plugin Execution

Execute Lua plugins when rule matches:

```toml
plugins = ["arm_notifier", "webhook_example"]
```

Plugins run before the action is applied.

---

## Modifier System

Modifiers are Lua scripts that transform MAVLink messages. They work with ALL 300+ MAVLink message types through automatic serialization.

### Modifier Structure

```lua
function modify(ctx)
    -- Access the message
    local msg = ctx.message
    
    -- ctx.message_type tells you what kind of message it is
    if ctx.message_type == "COMMAND_LONG" then
        -- Access fields directly from msg
        log.info(string.format("COMMAND_LONG: command=%s, target_system=%d",
            tostring(msg.command), msg.target_system))
        
        -- Modify fields
        msg.param1 = msg.param1 * 1.5
    end
    
    if ctx.message_type == "HEARTBEAT" then
        -- Note: base_mode is a table with 'bits' field
        if msg.base_mode and msg.base_mode.bits then
            msg.base_mode.bits = msg.base_mode.bits | 128  -- Set armed bit
        end
    end
    
    if ctx.message_type == "GLOBAL_POSITION_INT" then
        -- Modify position data
        msg.alt = math.min(msg.alt, 100000)  -- Clamp altitude
    end
    
    -- Update the message
    ctx.message = msg
    return ctx
end
```

### Context Fields

- `ctx.system_id` - MAVLink header system ID
- `ctx.component_id` - MAVLink header component ID
- `ctx.message_type` - Message type name (e.g., "COMMAND_LONG")
- `ctx.message` - Message data as a Lua table with fields directly accessible

### Message Field Access

Message fields are accessed directly from `ctx.message`:

```lua
-- Correct:
local target = msg.target_system
local param1 = msg.param1

-- Wrong (old nested structure):
-- local target = msg.COMMAND_LONG.target_system
```

### Special Fields

Some fields are tables, not simple values:

**base_mode (HEARTBEAT):**
```lua
if msg.base_mode and msg.base_mode.bits then
    local mode = msg.base_mode.bits
    msg.base_mode.bits = mode | 128  -- Set armed bit
end
```

### Logging in Modifiers

```lua
log.info("Information message")
log.warn("Warning message")
log.error("Error message")
log.debug("Debug message")
```

### Loading Modifiers

1. Create `.lua` file in `modifiers/` directory
2. Add to `config.toml`:
```toml
[modifiers.load]
my_modifier = "my_modifier.lua"
```
3. Reference in rules:
```toml
[[rules]]
actions = ["modify", "forward"]
modifier = "my_modifier"
```

### Example Modifiers

See `modifiers/always_armed.lua` and `modifiers/generic_modifier.lua` for complete examples.

---

## Plugin System

Plugins are Lua scripts that execute when rules match, performing side effects like notifications or logging.

### Plugin Structure

```lua
function on_match(ctx)
    -- Your plugin logic here
    log.info(string.format("Plugin executed for system %d", ctx.system_id))
    
    -- Access message data
    local msg = ctx.message
    
    if ctx.message_type == "COMMAND_LONG" then
        local target = msg.target_system
        local param1 = msg.param1
        
        -- Perform actions based on command
        if msg.command == "MAV_CMD_COMPONENT_ARM_DISARM" and param1 == 1.0 then
            log.info("ARM command detected")
            -- Send notification, log to file, etc.
        end
    end
end
```

### Context Fields

- `ctx.system_id` - MAVLink header system ID
- `ctx.component_id` - MAVLink header component ID
- `ctx.message_type` - Message type name
- `ctx.message` - Full message data as a table

### Message Field Access

Same as modifiers - fields are accessed directly:

```lua
local msg = ctx.message
local target = msg.target_system  -- Correct
-- NOT: msg.COMMAND_LONG.target_system (old API)
```

### Available APIs

#### Logging
```lua
log.info("Information message")
log.warn("Warning message")
log.error("Error message")
log.debug("Debug message")
```

#### Serial Communication
```lua
-- Write raw data to serial port
serial.write(port, baudrate, data, timeout_ms)

-- Example:
serial.write("/dev/ttyUSB0", 57600, "d01d", 3000)

-- Write with automatic newline
serial.write_line("/dev/ttyUSB0", 57600, "command", 3000)
```

#### HTTP Requests
```lua
-- GET request
local response = http.get("https://api.example.com/data")
local response = http.get("https://api.example.com/data", {["Authorization"] = "Bearer token"})

-- POST request
local body = '{"key": "value"}'
local response = http.post("https://api.example.com/webhook", body)
local response = http.post("https://api.example.com/webhook", body, {["Content-Type"] = "application/json"})
```

#### Utility Functions
```lua
-- Sleep for milliseconds
util.sleep(1000)

-- File operations
util.file_write("/tmp/log.txt", "content")
local content = util.file_read("/tmp/log.txt")
```

### Loading Plugins

1. Create `.lua` file in `plugins/` directory
2. Add to `config.toml`:
```toml
[plugins.load]
my_plugin = "my_plugin.lua"
```
3. Reference in rules:
```toml
[[rules]]
plugins = ["my_plugin"]
```

### Example Plugins

See `plugins/arm_notifier.lua` and `plugins/webhook_example.lua` for complete examples.

---

## Advanced Examples

### Bidirectional ARM Command Handling

```toml
# Synchronize ARM commands from GCS across multiple drones
[[rules]]
message_type = "COMMAND_LONG"
command = "MAV_CMD_COMPONENT_ARM_DISARM"
actions = ["batch", "delay"]
batch_count = 2
batch_timeout_seconds = 60
batch_key = "arm_swarm"
delay_seconds = 5
auto_ack = true
plugins = ["arm_notifier"]
direction = "gcs_to_router"
description = "Sync ARM across 2 drones, delay 5s"

[rules.conditions]
param1 = 1.0  # Only ARM (DISARM passes through)
```

### Modify HEARTBEAT in Both Directions

```toml
# Make drones appear armed to GCS
[[rules]]
message_type = "HEARTBEAT"
actions = ["modify", "forward"]
modifier = "always_armed"
direction = "router_to_gcs"
description = "Show drones as always armed"

# Block GCS heartbeats to router
[[rules]]
message_type = "HEARTBEAT"
actions = ["block"]
direction = "gcs_to_router"
description = "Block GCS heartbeats"

[rules.conditions]
system_id = 255
```

### Priority-Based Exception Rules

```toml
# HIGH PRIORITY: Emergency landing passes immediately
[[rules]]
message_type = "COMMAND_LONG"
command = "MAV_CMD_NAV_LAND"
actions = ["forward"]
priority = 1000
description = "Emergency landing - immediate"

[rules.conditions]
param1 = 1.0  # Emergency flag

# LOW PRIORITY: Normal landing delayed
[[rules]]
message_type = "COMMAND_LONG"
command = "MAV_CMD_NAV_LAND"
actions = ["delay"]
delay_seconds = 3
priority = 10
description = "Normal landing - 3s delay"
```

### Block Specific System in Both Directions

```toml
[[rules]]
message_type = "COMMAND_LONG"
command = "MAV_CMD_NAV_TAKEOFF"
actions = ["block"]
direction = "both"
description = "Prevent system 1 from taking off"

[rules.conditions]
system_id = 1
```

### Modify GPS Data from Drones

```toml
[[rules]]
message_type = "GPS_RAW_INT"
actions = ["modify", "forward"]
modifier = "gps_modifier"
direction = "router_to_gcs"
description = "Modify GPS data before sending to GCS"

[rules.conditions]
fix_type = 3  # Only 3D fix
```

### Batch Mission Items

```toml
[[rules]]
message_type = "MISSION_ITEM_INT"
actions = ["batch"]
batch_count = 5
batch_timeout_seconds = 10
batch_key = "mission_sync"
direction = "gcs_to_router"
description = "Sync mission items across 5 drones"
```

### Chain Multiple Actions

```toml
[[rules]]
message_type = "COMMAND_LONG"
command = "MAV_CMD_COMPONENT_ARM_DISARM"
actions = ["modify", "batch", "delay", "forward"]
modifier = "arm_modifier"
batch_count = 3
batch_key = "arm_swarm"
delay_seconds = 5
plugins = ["arm_notifier", "webhook_example"]
auto_ack = true
direction = "gcs_to_router"
description = "Full pipeline: modify -> batch -> delay -> forward"

[rules.conditions]
param1 = 1.0
```

---

## Technical Details

### How It Works

1. **Config Loading**: Loads and validates `config.toml`, sorts rules by priority
2. **GCS Connection**: Listens on port 14550 for incoming GCS connections
3. **Router Connection**: Maintains connection to mavlink-router on port 14551
4. **Message Processing**:
   - **GCS → Router**: Parses MAVLink, applies rules with direction "gcs_to_router" or "both"
   - **Router → GCS**: Parses MAVLink, applies rules with direction "router_to_gcs" or "both"
5. **Rule Matching**: Checks conditions in priority order, first match wins
6. **Action Execution**: Forwards, delays (async), blocks, modifies, or batches messages
7. **Destination Handling**: Uses unified `Destination` enum for both Router and GCS

### Concurrency Model

- Each delayed message spawns an independent async task
- Delays are truly concurrent - multiple commands can be delayed simultaneously
- Non-delayed traffic flows without blocking
- Batch manager handles synchronization across multiple drones with timeouts

### Safety Features

- Invalid MAVLink messages are forwarded anyway (fail-open)
- Config validation prevents invalid rules at startup
- Both directions support full rule processing
- Automatic error handling and logging

### Code Structure

```
src/
├── main.rs       # Entry point and initialization
├── config.rs     # Configuration loading and validation
├── rules.rs      # Rule engine and message processing
├── proxy.rs      # UDP proxy and forwarding logic
├── batch.rs      # Batch synchronization manager
├── modifiers.rs  # Lua modifier management
└── plugins/
    └── api/      # Plugin API implementation
```

### Generic Message Support

BITCH supports ALL MAVLink message types (300+) through:
- **Automatic serde serialization** to Lua tables
- **Dynamic message parsing** using mavlink-rs
- **Generic rule matching** on message_type string
- **Field-level condition matching** via JSON serialization

No Rust code changes needed to support new message types!

---

## Development

### Building
```bash
cargo build
```

### Running with Debug Output
Edit `config.toml`:
```toml
[logging]
level = "debug"
```

Then run:
```bash
cargo run
```

### Running Tests
```bash
cargo test
```

### Adding New Message Types

No code changes needed! Just use the message type name in rules:

```toml
[[rules]]
message_type = "YOUR_NEW_MESSAGE_TYPE"
actions = ["forward"]
```

### Extending the Modifier API

Edit `src/modifiers.rs` to add new Lua APIs accessible to modifiers.

### Extending the Plugin API

Edit `src/plugins/api/mod.rs` to add new APIs for plugins.

### Testing Without Drones

Monitor traffic without actual drones:

```bash
# Terminal 1: Run BITCH
cargo run

# Terminal 2: Send test UDP packets
echo "test" | nc -u localhost 14550
```

---

## Troubleshooting

### GCS Can't Connect

- Ensure BITCH is running and listening on port 14550
- Check firewall settings: `sudo ufw allow 14550/udp`
- Verify correct IP address in Mission Planner
- Check logs for "GCS listening on" message

### No Traffic Forwarding

- Verify mavlink-router is running: `systemctl status mavlink-router`
- Check router is on port 14551: `netstat -tulpn | grep 14551`
- Verify router configuration: `/etc/mavlink-router/main.conf`
- Increase log level to `debug` to see packet details

### Rules Not Applying

- Check `message_type` matches actual MAVLink message (check logs)
- Verify `command` name is correct for COMMAND_LONG rules
- Check conditions match expected values (use `debug` logging)
- Verify rule `direction` matches message flow
- Check rule priority order - higher priority rules checked first
- Validate TOML syntax in config.toml

### Modifier Not Working

- Check modifier is loaded in `[modifiers.load]` section
- Verify modifier file exists in `modifiers/` directory
- Check for Lua syntax errors in logs
- Ensure `modify` function is defined
- Verify modifier is referenced in rule with `modifier = "name"`
- Check message field access is correct (direct access, not nested)

### Plugin Errors

- Check plugin is loaded in `[plugins.load]` section
- Verify plugin file exists in `plugins/` directory
- Check for Lua errors in logs
- Ensure `on_match` function is defined
- Verify message field access: use `msg.field` not `msg.MESSAGE_TYPE.field`
- Check context fields: use `ctx.system_id` not `ctx.target_system`

### Config Validation Errors

- Ensure all required fields are present
- Check that `delay` action includes `delay_seconds`
- Verify `batch` action includes `batch_count` and `batch_timeout_seconds`
- Verify `modify` action includes `modifier` name
- Check action names: delay, block, forward, modify, batch
- Verify direction: gcs_to_router, router_to_gcs, both

### Performance Issues

- Reduce log level from `debug` to `info`
- Check for rules that match too frequently (e.g., HEARTBEAT)
- Verify delayed messages aren't backing up
- Monitor CPU usage: `top -p $(pgrep bitch)`

### Batch Not Synchronizing

- Verify `batch_count` matches number of expected drones
- Check `batch_timeout_seconds` is sufficient
- Ensure `batch_key` is unique for each batch group
- Check logs for "Created new batch group" messages
- Verify conditions correctly filter messages for batching

---

## Example Output

```
2025-11-13T09:10:41.804042Z  INFO BITCH MAVLINK Interceptor starting...
2025-11-13T09:10:41.804055Z  INFO    GCS listening on 0.0.0.0:14550
2025-11-13T09:10:41.804063Z  INFO    Router at 127.0.0.1:14551
2025-11-13T09:10:41.804070Z  INFO    Rules loaded: 1
2025-11-13T09:10:41.804104Z  INFO    - COMMAND_LONG (MAV_CMD_COMPONENT_ARM_DISARM) -> batch -> delay (5s)
2025-11-13T09:10:41.804182Z  INFO Sockets initialized
2025-11-13T09:10:41.804441Z  INFO GCS->Router forwarding started
2025-11-13T09:10:41.804516Z  INFO Router->GCS forwarding started
2025-11-13T09:10:42.102389Z  INFO GCS connected from: 127.0.0.1:14555
2025-11-13T09:10:48.102361Z  INFO Rule matched: COMMAND_LONG (MAV_CMD_COMPONENT_ARM_DISARM) - Synchronize ARM commands across 2 drones, then delay 5s before arming
2025-11-13T09:10:48.102673Z  INFO [Plugin] ARM detected for system 102
2025-11-13T09:10:48.107362Z  INFO [Plugin] Serial notification sent successfully
2025-11-13T09:10:48.107519Z  INFO Sent COMMAND_ACK to GCS (sysid=102, cmd=MAV_CMD_SET_MESSAGE_INTERVAL)
2025-11-13T09:10:48.107584Z  INFO Created new batch group 'arm_swarm' (threshold=2, timeout=60s)
```

---

## License

See LICENSE file for details.
