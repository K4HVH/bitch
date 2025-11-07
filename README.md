# BITCH
Basic Intercept & Transform Command Handler

A MAVLINK command interceptor and modifier that sits between a Ground Control Station (GCS) and mavlink-router, allowing you to filter, delay, or block specific MAVLINK commands using a powerful rule-based system.

## Architecture

```
Mission Planner (GCS) <--> BITCH <--> MAVLINK-Router <--> Drones
                      :14550            :14551
```

## Features

- **Transparent bidirectional UDP proxy** between GCS and mavlink-router
- **MAVLINK message parsing** (supports both v1 and v2)
- **Rule-based filtering system** with conditions and priorities
- **Flexible actions**: delay, block, forward, or modify (future)
- **Per-command rule configuration** via TOML file
- **Conditional matching** on parameters, system ID, component ID
- **Priority-based rule processing** for complex logic
- **Async/non-blocking operation** using Tokio
- **Modular codebase** for easy extension
- **Detailed logging** with tracing

## Installation

Build the project:
```bash
cargo build --release
```

The binary will be available at `target/release/bitch`

## Rule-Based System

BITCH uses a powerful rule-based system to process MAVLINK messages. Rules are defined in `config.toml` and are evaluated in priority order (highest priority first).

### Rule Structure

```toml
[[rules]]
message_type = "COMMAND_LONG"    # Type of MAVLINK message
command_id = 400                 # Optional: specific command ID
action = "delay"                 # Action: delay, block, forward, modify
delay_seconds = 10               # Optional: for delay action
priority = 50                    # Optional: higher = checked first (default: 0)
description = "..."              # Optional: human-readable description

[rules.conditions]
param1 = 1.0                     # Optional: match param1 value
system_id = 1                    # Optional: match system ID
# ... more conditions
```

### Available Fields

#### Message Types
- `COMMAND_LONG` - Command messages (most common)
- `HEARTBEAT` - Heartbeat messages
- `MISSION_ITEM` - Mission waypoint items
- `MISSION_REQUEST` - Mission requests
- `PARAM_REQUEST_READ` - Parameter read requests
- And many more (see `src/rules.rs::get_message_name()`)

#### Actions
- `delay` - Delay the message by specified seconds (requires `delay_seconds`)
- `block` - Block the message completely (never forward)
- `forward` - Forward immediately (explicit pass-through)
- `modify` - Future: modify message before forwarding

#### Conditions (all optional)
- `param1` through `param7` - Match specific parameter values (float)
- `system_id` - Match specific system ID (u8)
- `component_id` - Match specific component ID (u8)

#### Priority
- Higher numbers are checked first
- Default is 0
- Use priorities to create "exception" rules

### Example Configuration

```toml
[network]
gcs_listen_port = 14550
gcs_listen_address = "0.0.0.0"
router_address = "127.0.0.1"
router_port = 14551

[logging]
level = "info"

# HIGH PRIORITY: Let DISARM pass immediately (exception to ARM delay)
[[rules]]
message_type = "COMMAND_LONG"
command_id = 400  # MAV_CMD_COMPONENT_ARM_DISARM
action = "forward"
priority = 100
description = "DISARM commands pass immediately"

[rules.conditions]
param1 = 0.0  # 0 = DISARM

# MAIN RULE: Delay ARM commands by 10 seconds
[[rules]]
message_type = "COMMAND_LONG"
command_id = 400
action = "delay"
delay_seconds = 10
priority = 50
description = "Delay ARM commands by 10 seconds"

[rules.conditions]
param1 = 1.0  # 1 = ARM
```

### Common Command IDs

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

## Usage

### 1. Start mavlink-router

Your mavlink-router should already be running with the configuration at `/etc/mavlink-router/main.conf`

### 2. Run BITCH

```bash
cargo run --release
```

Or run the binary directly:
```bash
./target/release/bitch
```

### 3. Configure Mission Planner

In Mission Planner:
1. Go to connection settings
2. Set UDP connection to: `127.0.0.1:14550` (or your machine's IP if remote)
3. Connect

## Logging

Set the log level in `config.toml`:
- `trace` - Very verbose, shows all packet details
- `debug` - Shows parsed messages and forwarding
- `info` - Shows connections and rule matches (default)
- `warn` - Shows warnings and blocked messages
- `error` - Only errors

## Example Output

```
🚀 BITCH MAVLINK Interceptor starting...
   GCS listening on 0.0.0.0:14550
   Router at 127.0.0.1:14551
   Rules loaded: 2
   - COMMAND_LONG (ID: 400) -> forward
   - COMMAND_LONG (ID: 400) -> delay (10s)
✅ Sockets initialized
📡 GCS->Router forwarding started
📡 Router->GCS forwarding started
🎯 GCS connected from: 192.168.1.100:14560
🎯 Rule matched: COMMAND_LONG (ID: 400) - Delay ARM commands by 10 seconds
⏱️  Message queued for 10s delay (other traffic continues)
✅ Delayed message forwarded after 10s
```

## Advanced Examples

### Block All Mode Changes
```toml
[[rules]]
message_type = "COMMAND_LONG"
command_id = 176  # MAV_CMD_DO_SET_MODE
action = "block"
description = "Block all mode changes"
```

### Delay Mission Start
```toml
[[rules]]
message_type = "COMMAND_LONG"
command_id = 300  # MAV_CMD_MISSION_START
action = "delay"
delay_seconds = 5
description = "5 second safety delay before mission start"
```

### Block Takeoff from Specific System
```toml
[[rules]]
message_type = "COMMAND_LONG"
command_id = 22  # MAV_CMD_NAV_TAKEOFF
action = "block"
description = "Prevent system 1 from taking off"

[rules.conditions]
system_id = 1
```

### Complex Priority Example
```toml
# Allow emergency landing immediately (highest priority)
[[rules]]
message_type = "COMMAND_LONG"
command_id = 21  # MAV_CMD_NAV_LAND
action = "forward"
priority = 1000
description = "Emergency landing - immediate"

[rules.conditions]
param1 = 1.0  # Emergency flag

# Delay normal landing by 3 seconds
[[rules]]
message_type = "COMMAND_LONG"
command_id = 21
action = "delay"
delay_seconds = 3
priority = 10
description = "Normal landing - 3s delay"
```

## Code Structure

The codebase is organized into modular components:

```
src/
├── main.rs       # Entry point and initialization
├── config.rs     # Configuration loading and validation
├── rules.rs      # Rule engine and message processing
└── proxy.rs      # UDP proxy and forwarding logic
```

### Extending the System

#### Adding New Message Types

Edit `src/rules.rs::get_message_name()`:
```rust
pub fn get_message_name(msg: &MavMessage) -> &'static str {
    match msg {
        MavMessage::YOUR_NEW_TYPE(_) => "YOUR_NEW_TYPE",
        // ... existing matches
    }
}
```

#### Adding New Command IDs

Edit `src/rules.rs::get_command_id()`:
```rust
fn get_command_id(cmd: &MavCmd) -> u16 {
    match cmd {
        MavCmd::YOUR_NEW_COMMAND => 999,
        // ... existing matches
    }
}
```

#### Adding Custom Conditions

Future support for custom conditions can be added through the `custom` field in `RuleConditions`.

## Troubleshooting

### GCS can't connect
- Ensure BITCH is running and listening on port 14550
- Check firewall settings
- Verify the correct IP address in Mission Planner

### No traffic forwarding
- Check that mavlink-router is running on port 14551
- Verify router configuration at `/etc/mavlink-router/main.conf`
- Increase log level to `debug` to see packet details

### Rules not applying
- Check that `message_type` and `command_id` match the actual MAVLINK message
- Verify conditions match expected values (use `debug` logging)
- Check rule priority order - higher priority rules are checked first
- Verify TOML syntax in config.toml

### Config validation errors
- Ensure all required fields are present
- Check that `delay` action includes `delay_seconds`
- Verify action is one of: delay, block, forward, modify

## Development

### Building
```bash
cargo build
```

### Running with debug output
```bash
# Edit config.toml and set: level = "debug"
cargo run
```

### Running tests
```bash
cargo test
```

### Testing the proxy
You can test without drones by monitoring traffic:
```bash
# In one terminal
cargo run

# In another terminal, send test UDP packets
echo "test" | nc -u localhost 14550
```

## Technical Details

### How It Works

1. **Config Loading**: Loads and validates `config.toml`, sorts rules by priority
2. **GCS Connection**: Listens on port 14550 for incoming GCS connections
3. **Router Connection**: Maintains connection to mavlink-router on port 14551
4. **Message Processing**:
   - GCS → Router: Parses MAVLINK, applies rules, executes actions
   - Router → GCS: Transparent forwarding (no processing)
5. **Rule Matching**: Checks conditions in priority order, first match wins
6. **Action Execution**: Forwards, delays (async), or blocks messages

### Concurrency

- Each delayed message spawns an independent async task
- Delays are truly concurrent - multiple commands can be delayed simultaneously
- Non-delayed traffic flows without blocking

### Safety

- Invalid MAVLINK messages are forwarded anyway (fail-open)
- Router → GCS direction is always transparent
- Config validation prevents invalid rules

## License

See LICENSE file for details.
