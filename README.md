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
QGC             ┐
Mission Planner ├─→ BITCH :5760 (TCP) ←→ mavlink-router :5761 (TCP) ←→ Drones
Other GCS       ┘
```

**Multi-Client Support**: BITCH accepts unlimited simultaneous GCS connections on port 5760. All clients receive identical telemetry (broadcast), and commands from any client are processed through the rule engine.

## Configuration

Edit `config.toml` to define rules for intercepting and transforming MAVLink messages.

See `DOCUMENTATION.md` for complete documentation including:
- Rule system with conditions and priorities
- **Trigger system** (rules can activate/deactivate other rules)
- Modifiers (Lua scripts for transforming messages)
- Plugins (Lua scripts for side effects)
- Batch synchronization across drones
- Command chaining (actions execute sequentially)
- Auto-ACK system (generic for all message types)
- Bidirectional message processing
- All 300+ MAVLink message types

## Features

- **Multi-client TCP proxy** - Unlimited simultaneous GCS connections
- Bidirectional TCP forwarding (GCS ↔ Router)
- **Broadcast telemetry** - All GCS clients receive identical data
- Rule-based message filtering with priorities
- **Trigger system** - Dynamic rule activation/deactivation
- Actions: forward, block, modify, delay, batch (with command chaining)
- Direction control: gcs_to_router, router_to_gcs, both
- Auto-ACK system (generic for all message types)
- Lua scripting for modifiers and plugins
- **Cross-client batch synchronization** - Commands from different GCS apps contribute to same batch
- Async/non-blocking operation
- Graceful client connect/disconnect handling

## License

See LICENSE file for details.
