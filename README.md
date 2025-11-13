# BITCH

**B**asic **I**ntercept & **T**ransform **C**ommand **H**andler

A MAVLink proxy that intercepts and transforms messages between Ground Control Station (GCS) and drones via mavlink-router.

## Quick Start

```bash
# Build
cargo build --release

# Run
./target/release/bitch
```

## Architecture

```
Mission Planner <--> BITCH :14550 <--> mavlink-router :14551 <--> Drones
```

## Configuration

Edit `config.toml` to define rules for intercepting and transforming MAVLink messages.

See `DOCUMENTATION.md` for complete documentation including:
- Rule system with conditions and priorities
- Modifiers (Lua scripts for transforming messages)
- Plugins (Lua scripts for side effects)
- Batch synchronization across drones
- Bidirectional message processing
- All 300+ MAVLink message types

## Features

- Bidirectional UDP proxy (GCS ↔ Router)
- Rule-based message filtering
- Actions: forward, block, modify, delay, batch
- Direction control: gcs_to_router, router_to_gcs, both
- Lua scripting for modifiers and plugins
- Priority-based rule processing
- Async/non-blocking operation

## License

See LICENSE file for details.
