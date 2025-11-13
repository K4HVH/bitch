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
6. [Trigger System](#trigger-system)
7. [Modifier System](#modifier-system)
8. [Plugin System](#plugin-system)
9. [Advanced Examples](#advanced-examples)
10. [Technical Details](#technical-details)
11. [Development](#development)
12. [Troubleshooting](#troubleshooting)

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
- **Trigger system** - Rules can activate/deactivate other rules dynamically
- **Flexible actions**: delay, block, forward, modify, batch
- **Generic message support** - Works with ALL 300+ MAVLink message types
- **Conditional matching** on ANY message field
- **Priority-based rule processing** for complex logic
- **Direction control**: Apply rules to GCS→Router, Router→GCS, or both
- **Lua scripting** for modifiers and plugins (unified API)
- **Batch synchronization** across multiple drones with configurable field extraction
- **Generic Auto-ACK** for ANY message type (not just COMMAND_LONG)
- **Command chaining** - Actions execute sequentially through the entire chain
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
name = "my_rule"                 # Unique rule identifier (required)
message_type = "MESSAGE_TYPE"    # Which message type to match (required)
priority = 10                    # Higher = checked first (default: 0)
actions = ["action1", "action2"] # Sequential actions to apply (required)
direction = "gcs_to_router"      # Flow direction (default: "gcs_to_router")
enabled_by_default = true        # Whether rule is active on startup (default: true)
description = "What this rule does"

[rules.conditions]               # Match specific message fields (optional)
command = { type = "MAV_CMD_..." }  # Match command field (internally-tagged enum format)
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

[rules.ack]
message_type = "COMMAND_ACK"                    # Which message type to send as ACK
source_system_field = "target_system"           # Field to use as ACK source system_id
source_component_field = "target_component"     # Field to use as ACK source component_id

# Fields to set in ACK message (enums use internally-tagged format)
fields = { result = { type = "MAV_RESULT_ACCEPTED" } }

# Optional: Copy fields from original message to ACK
copy_fields = { command = "command", target_system = "header.system_id" }
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

[rules.conditions]
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }

[rules.ack]
message_type = "COMMAND_ACK"
source_system_field = "target_system"
source_component_field = "target_component"
fields = { result = { type = "MAV_RESULT_ACCEPTED" } }
copy_fields = { command = "command", target_system = "header.system_id", target_component = "header.component_id" }
```

**Example - MISSION_ACK for MISSION_REQUEST_LIST:**
```toml
[[rules]]
message_type = "MISSION_REQUEST_LIST"
auto_ack = true

[rules.ack]
message_type = "MISSION_COUNT"
source_system_field = "target_system"
source_component_field = "target_component"
fields = { count = 0, mission_type = { type = "MAV_MISSION_TYPE_MISSION" } }
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
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }  # Enum (internally-tagged)
param1 = 1.0
param2 = 0.0
target_system = 1
target_component = 1

# GPS_RAW_INT fields
fix_type = 3
satellites_visible = 10

# HEARTBEAT fields
system_status = { type = "MAV_STATE_ACTIVE" }  # Enum (internally-tagged)
autopilot = { type = "MAV_AUTOPILOT_ARDUPILOTMEGA" }  # Enum (internally-tagged)

# GLOBAL_POSITION_INT fields
alt = 1000

# RC_CHANNELS fields
chan1_raw = 1500

# ANY field in ANY message type - completely generic!
```

**Field matching supports:**
- **Integers**: Exact match
- **Floats**: Within epsilon
- **Strings**: Exact match
- **Booleans**: Exact match
- **Enums**: Internally-tagged format (e.g., `{ type = "MAV_CMD_..." }`)

---

## Trigger System

The trigger system allows rules to dynamically activate or deactivate other rules when they match. This enables powerful conditional logic like "when drones ARM, activate the always_armed modifier for 60 seconds."

### Overview

**Key Features:**
- Rules can activate/deactivate other rules by name
- Time-limited activations with automatic expiration
- Multiple rules can be triggered simultaneously
- Cascading triggers (triggered rules can have their own triggers)
- Configurable timing (on match or on complete)

**Use Cases:**
- Temporarily enable rules for a duration after an event
- Create state machines with rule activation chains
- Conditionally enable modifiers based on commands
- Coordinate multiple rule behaviors dynamically

### Trigger Configuration

Add a `[rules.triggers]` section to any rule:

```toml
[[rules]]
name = "arm_command"
message_type = "COMMAND_LONG"
actions = ["batch", "delay"]
# ... other rule config ...

[rules.triggers]
activate_rules = ["show_always_armed", "extra_logging"]  # Rules to enable
deactivate_rules = ["block_telemetry"]                   # Rules to disable
duration_seconds = 60                                     # How long to keep activated
on_match = true                                           # Trigger when rule matches (default)
on_complete = false                                       # Trigger after actions complete (default)

[rules.conditions]
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }
param1 = 1.0
```

### Trigger Fields

#### activate_rules (array of strings)
List of rule names to enable when this rule triggers.

```toml
activate_rules = ["rule1", "rule2", "rule3"]
```

- Rules must exist (validated at startup)
- Multiple rules can be activated simultaneously
- Activated rules start processing immediately

#### deactivate_rules (array of strings)
List of rule names to disable when this rule triggers.

```toml
deactivate_rules = ["rule_to_stop"]
```

- Immediately disables the specified rules
- Rules stop matching new messages
- Does not affect already-queued messages in delays/batches

#### duration_seconds (optional integer)
How long to keep activated rules enabled (in seconds).

```toml
duration_seconds = 60  # Keep active for 60 seconds
```

- Only applies to `activate_rules`
- After duration expires, rules automatically deactivate
- Background cleanup task runs every 1 second
- If omitted, activated rules stay enabled permanently

#### on_match (boolean, default: true)
Trigger when the rule matches a message.

```toml
on_match = true  # Trigger immediately when rule matches
```

- Triggers before actions execute
- Useful for event-based activation

#### on_complete (boolean, default: false)
Trigger after all actions complete.

```toml
on_complete = true  # Trigger after delay/batch finishes
```

- Triggers after the entire action chain completes
- Useful for sequencing rules
- **Note:** Currently only `on_match` is fully implemented

### Rule State Management

#### enabled_by_default (boolean, default: true)
Whether a rule is active when BITCH starts.

```toml
[[rules]]
name = "triggered_rule"
enabled_by_default = false  # Disabled until another rule activates it
# ... rest of rule config ...
```

- `true`: Rule is active on startup (default)
- `false`: Rule is disabled until explicitly activated by a trigger
- Useful for rules that should only run conditionally

#### Rule Names (required)
Every rule must have a unique name.

```toml
[[rules]]
name = "my_unique_rule"  # Required for trigger system
```

- Used to reference rules in triggers
- Must be unique across all rules
- Validated at startup (error if duplicate names)

### Timer Behavior

**Multiple Triggers:**
When a rule is triggered multiple times, **the timer resets**.

Example:
```
T=0s:  ARM command 1 → activates "show_always_armed" until T=60s
T=5s:  ARM command 2 → resets timer, now active until T=65s
T=10s: ARM command 3 → resets timer, now active until T=70s
```

The rule stays active until `duration_seconds` after the **last** trigger.

**Auto Cleanup:**
- Background task runs every 1 second
- Expired rule activations are automatically removed
- Deactivated rules stop processing immediately
- Logged with debug messages

### Cascading Triggers

Triggered rules can have their own triggers, creating chains:

```toml
[[rules]]
name = "initial_rule"
message_type = "COMMAND_LONG"
actions = ["forward"]

[rules.triggers]
activate_rules = ["second_rule"]
on_match = true

[rules.conditions]
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }

[[rules]]
name = "second_rule"
message_type = "HEARTBEAT"
enabled_by_default = false
actions = ["modify", "forward"]
modifier = "always_armed"

[rules.triggers]
activate_rules = ["third_rule"]  # This rule can also trigger others!
duration_seconds = 30
on_match = true
```

### Complete Example: ARM-Activated Modifier

```toml
# Rule 1: ARM command handler with trigger
[[rules]]
name = "arm_sync"
message_type = "COMMAND_LONG"
actions = ["batch", "delay"]
batch_count = 2
batch_timeout_seconds = 60
batch_timeout_forward = true
batch_key = "arm_swarm"
batch_system_id_field = "target_system"
delay_seconds = 5
auto_ack = true
plugins = ["arm_notifier"]
direction = "gcs_to_router"
description = "Synchronize ARM commands across 2 drones, then delay 5s before arming"

[rules.conditions]
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }
param1 = 1.0

[rules.ack]
message_type = "COMMAND_ACK"
source_system_field = "target_system"
source_component_field = "target_component"
fields = { result = { type = "MAV_RESULT_ACCEPTED" } }
copy_fields = { command = "command", target_system = "header.system_id", target_component = "header.component_id" }

# Trigger: Activate always_armed rule for 60 seconds when drones ARM
[rules.triggers]
activate_rules = ["show_always_armed"]
duration_seconds = 60
on_match = true

# Rule 2: Always armed modifier (disabled by default, activated by trigger)
[[rules]]
name = "show_always_armed"
message_type = "HEARTBEAT"
actions = ["modify", "forward"]
modifier = "always_armed"
direction = "router_to_gcs"
enabled_by_default = false
description = "Show drones as always armed (activated by ARM trigger)"
```

**How it works:**
1. GCS sends ARM command
2. `arm_sync` rule matches
3. Trigger activates `show_always_armed` for 60 seconds
4. For 60 seconds, all HEARTBEAT messages show drones as armed
5. After 60 seconds, `show_always_armed` automatically deactivates
6. HEARTBEAT messages return to normal

### Manual Deactivation Example

Use deactivation rules to manually stop rules:

```toml
# Rule that activates on ARM
[[rules]]
name = "arm_handler"
message_type = "COMMAND_LONG"
actions = ["forward"]
direction = "gcs_to_router"

[rules.conditions]
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }
param1 = 1.0  # ARM

[rules.triggers]
activate_rules = ["show_always_armed"]
duration_seconds = 120
on_match = true

# Rule that deactivates on DISARM
[[rules]]
name = "disarm_handler"
message_type = "COMMAND_LONG"
actions = ["forward"]
direction = "gcs_to_router"

[rules.conditions]
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }
param1 = 0.0  # DISARM

[rules.triggers]
deactivate_rules = ["show_always_armed"]  # Stop showing armed immediately
on_match = true

# The modifier rule
[[rules]]
name = "show_always_armed"
message_type = "HEARTBEAT"
actions = ["modify", "forward"]
modifier = "always_armed"
direction = "router_to_gcs"
enabled_by_default = false
```

---

## Modifier System

Modifiers are Lua scripts that transform MAVLink messages before forwarding. They work with ALL 300+ message types!

### Modifier Structure

Modifiers must implement a `modify()` function:

```lua
function modify(ctx)
    local msg = ctx.message

    -- Messages use mavlink internally-tagged format: {type = "MESSAGE_TYPE", field1 = ..., field2 = ...}
    if msg.type == "COMMAND_LONG" then
        -- Modify fields directly on the message table
        msg.param1 = msg.param1 * 2
        log.info(string.format("Modified param1 for command %s", tostring(msg.command)))
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
    message = {                 -- Message data (mavlink internally-tagged format)
        type = "COMMAND_LONG",  -- Message type
        command = ...,          -- Fields directly accessible
        param1 = ...,
        target_system = ...,
        -- ... all fields at top level
    }
}
```

### Available APIs

```lua
log.info(message)    -- Logged with [Modifier] prefix
log.warn(message)    -- Logged with [Modifier] prefix
log.error(message)   -- Logged with [Modifier] prefix
log.debug(message)   -- Logged with [Modifier] prefix
```

### Examples

#### Example 1: Modify COMMAND_LONG parameters

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.type == "COMMAND_LONG" then
        -- Double param1 value
        msg.param1 = msg.param1 * 2

        log.info(string.format("Modified param1 for cmd %s", tostring(msg.command)))
    end

    return ctx
end
```

#### Example 2: Modify HEARTBEAT to show always armed

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.type == "HEARTBEAT" then
        -- base_mode is a table with 'bits' field for bitflags
        if msg.base_mode and msg.base_mode.bits then
            local armed_bit = 128  -- MAV_MODE_FLAG_SAFETY_ARMED
            msg.base_mode.bits = msg.base_mode.bits | armed_bit
            log.info(string.format("Set armed bit for system %d", ctx.system_id))
        end
    end

    return ctx
end
```

#### Example 3: Clamp altitude in GLOBAL_POSITION_INT

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.type == "GLOBAL_POSITION_INT" then
        -- Limit altitude to 100m
        if msg.alt > 100000 then  -- Altitude in millimeters
            msg.alt = 100000
            log.warn("Clamped altitude to 100m")
        end
    end

    return ctx
end
```

#### Example 4: Modify MISSION_ITEM_INT waypoint

```lua
function modify(ctx)
    local msg = ctx.message

    if msg.type == "MISSION_ITEM_INT" then
        -- Reduce all waypoint altitudes by 20%
        msg.z = msg.z * 0.8
        log.info(string.format("Modified waypoint #%d altitude", msg.seq))
    end

    return ctx
end
```

### Enum/Bitflag Handling

Some fields are enums or bitflags and serialize as internally-tagged tables:

```lua
-- Bitflags (base_mode, custom_mode, etc.)
if msg.base_mode and msg.base_mode.bits then
    local mode = msg.base_mode.bits
    msg.base_mode.bits = mode | 128  -- Set bit
end

-- Enums are internally-tagged: { type = "VARIANT_NAME" }
if msg.command and msg.command.type then
    local cmd_type = msg.command.type  -- e.g., "MAV_CMD_COMPONENT_ARM_DISARM"
    log.info(string.format("Command: %s", cmd_type))
end
```

---

## Plugin System

Plugins are Lua scripts that execute side effects when rules match. They have the same API as modifiers but DON'T return modified messages.

### Plugin Structure

Plugins must implement an `on_match()` function:

```lua
function on_match(ctx)
    local msg = ctx.message

    -- Messages use mavlink internally-tagged format: {type = "MESSAGE_TYPE", field1 = ..., field2 = ...}
    if msg.type == "COMMAND_LONG" then
        -- Do something (send notification, log, etc.)
        log.info(string.format("ARM command for system %d", msg.target_system))
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
    message = {                 -- Message data (mavlink internally-tagged format)
        type = "COMMAND_LONG",  -- Message type
        command = ...,          -- Fields directly accessible
        param1 = ...,
        target_system = ...,
        -- ... all fields at top level
    }
}
```

### Available APIs

**Logging:**
```lua
log.info(message)    -- Logged with [Plugin] prefix
log.warn(message)    -- Logged with [Plugin] prefix
log.error(message)   -- Logged with [Plugin] prefix
log.debug(message)   -- Logged with [Plugin] prefix
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

    if msg.type == "COMMAND_LONG" then
        -- Calculate drone ID
        local drone_id = (msg.target_system - 100) % 10
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

    if msg.type == "COMMAND_LONG" then
        -- Build JSON payload
        local payload = string.format([[{
            "event": "arm_command",
            "system_id": %d,
            "target_system": %d,
            "command": "%s",
            "timestamp": %d
        }]], ctx.system_id, msg.target_system,
            tostring(msg.command), os.time())

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

    if msg.type == "GPS_RAW_INT" then
        log.info(string.format("GPS Fix: type=%d, sats=%d, lat=%d, lon=%d",
            msg.fix_type, msg.satellites_visible, msg.lat, msg.lon))
    end
end
```

---

## Advanced Examples

### Example 1: Synchronize ARM Across Swarm with Trigger

Batch ARM commands from multiple drones, delay, then forward, and activate a modifier:

```toml
[[rules]]
name = "arm_swarm_sync"
message_type = "COMMAND_LONG"
actions = ["batch", "delay"]
batch_count = 3                          # Wait for 3 drones
batch_timeout_seconds = 30
batch_timeout_forward = true
batch_key = "arm_swarm"
batch_system_id_field = "target_system"  # Batch by target (recipient)
delay_seconds = 5
auto_ack = true
plugins = ["arm_notifier"]
direction = "gcs_to_router"
description = "Synchronize ARM across 3 drones with 5s delay"

[rules.conditions]
command = { type = "MAV_CMD_COMPONENT_ARM_DISARM" }
param1 = 1.0  # Only ARM (not DISARM)

[rules.ack]
message_type = "COMMAND_ACK"
source_system_field = "target_system"
source_component_field = "target_component"
fields = { result = { type = "MAV_RESULT_ACCEPTED" } }
copy_fields = { command = "command" }

[rules.triggers]
activate_rules = ["show_armed_status"]
duration_seconds = 120
on_match = true
```

### Example 2: Block Emergency LAND Commands

Prevent emergency land from specific system:

```toml
[[rules]]
name = "block_emergency_land"
message_type = "COMMAND_LONG"
actions = ["block"]
priority = 100  # High priority
direction = "gcs_to_router"
description = "Block emergency LAND from system 255"

[rules.conditions]
command = { type = "MAV_CMD_NAV_LAND" }
param1 = 1.0  # Emergency flag
system_id = 255  # GCS
```

### Example 3: Modify HEARTBEAT from Drones

Make all drones appear armed:

```toml
[[rules]]
name = "show_always_armed"
message_type = "HEARTBEAT"
actions = ["modify", "forward"]
modifier = "always_armed"
direction = "router_to_gcs"
enabled_by_default = false  # Only enable when triggered
description = "Show drones as always armed"
```

### Example 4: Delay GPS Data with 3D Fix

Add 1 second delay to GPS messages:

```toml
[[rules]]
name = "delay_gps_fix"
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
name = "block_errors"
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
name = "mission_sync"
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
name = "ack_mission_requests"
message_type = "MISSION_REQUEST_LIST"
actions = ["delay"]
delay_seconds = 2
auto_ack = true
direction = "gcs_to_router"
description = "ACK mission requests"

[rules.ack]
message_type = "MISSION_COUNT"
source_system_field = "target_system"
source_component_field = "target_component"
fields = { count = 0, mission_type = { type = "MAV_MISSION_TYPE_MISSION" } }
```

---

## Technical Details

### Message Flow

```
1. UDP Packet Received
2. Parse MAVLink (v2/v1)
3. Extract message type
4. Find matching rule (priority order)
   - Check if rule is enabled
   - Check direction
   - Check message_type
   - Check conditions (ALL fields generic)
5. Execute triggers (if on_match = true)
   - Activate/deactivate rules
   - Set expiration timers
6. Execute plugins (if any)
7. Build action sequence
8. Execute modifiers (if modify action)
9. Send ACK (if auto_ack)
10. Execute actions recursively (command chaining):
    - Forward → send packet
    - Block → drop packet
    - Modify → reconstruct packet
    - Delay → spawn async task
    - Batch → queue or release, then continue chain
    - All actions maintain the remaining action chain
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
- If `batch_timeout_forward = true`: Forward all packets and apply remaining actions in the chain
- If `batch_timeout_forward = false`: Drop all packets
- Warning logged with statistics
- Remaining actions (like delay) are applied even on timeout!

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
// Serialize message to JSON (mavlink internally-tagged format)
let msg_json = serde_json::to_value(msg)?;

// Extract any field directly
let field_value = msg_json.get("field_name")?;

// Enum fields are internally-tagged: {"type": "VARIANT_NAME"}
if let Some(enum_obj) = field_value.as_object() {
    if let Some(enum_type) = enum_obj.get("type") {
        // This is an enum variant
    }
}
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
- **Check if rule is disabled** - Look for "Rule 'X' is disabled, skipping" in logs
- Verify `enabled_by_default = true` or that rule has been activated by a trigger

### Trigger System Issues

- **Rule not activating:** Check trigger rule is matching (look for "Rule matched" logs)
- **Duplicate rule names:** All rules must have unique names (error on startup)
- **Referenced rule doesn't exist:** Triggers validate at startup - check for errors
- **Timer not expiring:** Background cleanup runs every 1 second, check logs for "Rule 'X' activation expired"
- **Rule stays active too long:** Each trigger resets the timer - check if rule is being repeatedly triggered
- **Cascading triggers not working:** Ensure triggered rules have `enabled_by_default = false` and are being activated

### Lua Script Errors

- Check logs for Lua error messages (look for `[Plugin]` or `[Modifier]` prefixes)
- Verify script syntax: `lua -c script.lua`
- Ensure message structure access is correct (use `msg.type == "MESSAGE_TYPE"`)
- Access fields directly on msg table (not nested under MESSAGE_TYPE)
- Check that you're returning `ctx` from `modify()` functions

### Auto-ACK Not Working

- Verify `auto_ack = true`
- Check `[rules.ack]` section exists with `message_type`
- Verify `source_system_field` and `source_component_field` exist in message
- Check enum fields in `fields` use internally-tagged format: `{ type = "VARIANT" }`
- Look for "Failed to build ACK" or "Failed to deserialize ACK" errors in logs

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
