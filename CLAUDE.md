# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Chao-OS is a microkernel simulation written in Rust. The architecture is strictly organized into four layers with a central asynchronous message bus for all inter-module communication.

## Architecture Layers

```
┌─────────────────────────────────────────────────────┐
│              User Interaction Layer (UI)             │
│  • CLI Shell (user commands)                         │
│  • TUI System Monitor                                │
└────────────────────┬────────────────────────────────┘
                     │
┌────────────────────┴────────────────────────────────┐
│            Exchange Layer (Message Bus)              │
│  • KernelMsg enum (all communication)                │
│  • LockedBus (Mutex-based implementation)            │
│  • LockFreeBus (Atomic-based implementation)         │
└────────────────────┬────────────────────────────────┘
                     │
┌────────────────────┴────────────────────────────────┐
│              Service Layer (Kernel Services)         │
│  • Process Service (PCB/TCB management)              │
│  • Storage Service (paging/Swap)                     │
│  • File Service                                      │
│  • Device Service                                    │
└────────────────────┬────────────────────────────────┘
                     │
┌────────────────────┴────────────────────────────────┐
│               Hardware Layer (Simulation)            │
│  • Virtual CPU (Mock ISA)                           │
│  • Virtual MMU                                       │
│  • Virtual Disk                                      │
│  • Timer (Clock)                                     │
└─────────────────────────────────────────────────────┘
```

## Communication Protocol

- **Pattern**: Fire-and-forget asynchronous messaging
- **Medium**: All inter-module communication MUST use the `KernelMsg` enum
- **Routing**: The message bus routes `KernelMsg` to appropriate services based on message type
- **Strict Rule**: NO direct function calls across module boundaries. Use channels via the message bus only.

## Core Type Definitions

### `KernelMsg` Enum
Central enum covering all message types:
- Syscall requests
- Hardware interrupts
- Process service requests
- Storage service requests
- File service requests
- Device service requests

### `MessageBus` Trait
```rust
trait MessageBus {
    fn send(&self, msg: KernelMsg);
}
```

### Implementations
- `LockedBus`: Mutex-based message bus implementation (reference implementation)
- `LockFreeBus`: Atomic-based implementation (future)

## Common Development Commands

```bash
# Build the project
cargo build

# Run with output
cargo run

# Check code without building
cargo check

# Run tests
cargo test

# Run a specific test
cargo test test_name

# Run with logging
RUST_LOG=debug cargo run

# Format code
cargo fmt

# Lint code
cargo clippy
```

## Core Data Structures

These structures MUST derive `Debug` and `Clone`:
- `PCB` (Process Control Block)
- `TCB` (Thread Control Block)
- `PageTable`
- Other core kernel structures

## Coding Standards

- Use idiomatic Rust patterns
- Avoid `unsafe` unless absolutely necessary
- All modules communicate via channels through the message bus
- No direct cross-module function calls
- Prefer composition over inheritance
- Use `Result` types for error handling
