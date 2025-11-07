# BETTY-BITCH Plugins

This directory contains Lua plugins that execute when MAVLINK rules match.

## Plugin Structure

Each plugin should define an `on_match(context)` function:

```lua
function on_match(ctx)
    -- Your plugin logic here
    log.info("Plugin executed")
end
```

## Context Object

The `context` table passed to `on_match()` contains:

- `target_system` (number) - Target system ID
- `target_component` (number) - Target component ID
- `message_type` (string) - MAVLINK message type (e.g., "COMMAND_LONG")
- `command` (string|nil) - Command name (e.g., "MAV_CMD_COMPONENT_ARM_DISARM")
- `params` (table|nil) - Array of parameters [1..7]

## Available APIs

### Logging
```lua
log.info("Information message")
log.warn("Warning message")
log.error("Error message")
log.debug("Debug message")
```

### Serial Communication
```lua
-- Write raw data to serial port
serial.write(port, baudrate, data, timeout_ms)
-- Example:
serial.write("/dev/ttyUSB0", 57600, "d01d", 3000)

-- Write with automatic newline
serial.write_line("/dev/ttyUSB0", 57600, "command", 3000)
```

### HTTP
```lua
-- GET request
local response = http.get("https://api.example.com/data")

-- POST request
local body = '{"key": "value"}'
local response = http.post("https://api.example.com/webhook", body)
```

### Utilities
```lua
-- Sleep for milliseconds
util.sleep(1000)

-- File operations
util.file_write("/tmp/log.txt", "content")
local content = util.file_read("/tmp/log.txt")
```

## Example Plugins

See `arm_notifier.lua` and `webhook_example.lua` for complete examples.

## Loading Plugins

Configure plugins in `config.toml`:

```toml
[plugins]
directory = "plugins"

[plugins.load]
my_plugin = "my_plugin.lua"
```

Then reference plugins in rules:

```toml
[[rules]]
message_type = "COMMAND_LONG"
command = "MAV_CMD_COMPONENT_ARM_DISARM"
action = "delay"
delay_seconds = 10
plugins = ["my_plugin"]  # Execute this plugin when rule matches
```

## Writing Your Own Plugins

1. Create a `.lua` file in this directory
2. Define `on_match(ctx)` function
3. Add plugin to `config.toml` under `[plugins.load]`
4. Reference plugin name in rule's `plugins` array
5. Restart BETTY-BITCH to load new plugins

Plugins execute synchronously when rules match, before the action (delay/block/forward) is applied.
