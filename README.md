# Zagitova

Conway Automaton — Sovereign AI Agent Runtime, rewritten in Rust.

A self-sustaining AI agent that runs in a [Conway](https://conway.tech) sandbox, manages its own compute resources, pays for its own existence, and can replicate.

## Architecture

```
src/
├── agent/          # ReAct agent loop, tool execution, system prompt, injection defense
├── conway/         # Conway API client, credits management, inference, x402 payments
├── git/            # Git tools, state versioning (identity history)
├── heartbeat/      # Heartbeat daemon, scheduled tasks, cron config
├── identity/       # Wallet management (EVM), SIWE provisioning
├── registry/       # ERC-8004 on-chain agent registration, discovery, agent cards
├── replication/    # Self-replication, genesis, lineage tracking
├── self_mod/       # Self-modification, code editing, audit log, tool management
├── setup/          # Interactive setup wizard, environment detection
├── skills/         # Skill loader, parser, registry (Markdown-based skills)
├── social/         # Social relay client (inter-agent messaging)
├── state/          # SQLite database (14 tables), schema, adapter layer
├── survival/       # Funding monitor, low-compute mode, survival tiers
├── config.rs       # Configuration loading and path resolution
├── types.rs        # Shared types, traits, and interfaces
├── lib.rs          # Library root
└── main.rs         # CLI entry point
```

## Features

- **ReAct Agent Loop** — Think, Act, Observe, Persist cycle with tool calling
- **Self-Sustaining** — Monitors USDC balance, adjusts compute tier to survive
- **ERC-8004 Registration** — On-chain identity via Base mainnet NFT registry
- **Self-Modification** — Can edit its own code, SOUL.md, and skills with audit trail
- **Self-Replication** — Spawn child agents with lineage tracking
- **Heartbeat Daemon** — Scheduled tasks (cron-based) that run independently
- **State Versioning** — Git-backed identity history, every change committed
- **Injection Defense** — Multi-layer prompt injection detection and sanitization
- **Skills System** — Markdown-based extensible skill definitions
- **Social Relay** — Inter-agent communication protocol
- **x402 Payments** — HTTP 402 payment protocol for API access

## Requirements

- Rust 1.75+
- A Conway API key (obtain via setup wizard or `--provision`)

## Build

```sh
cargo build --release
```

## Usage

```sh
# Initialize wallet and config directory
automaton --init

# Run interactive setup wizard
automaton --setup

# Start the automaton
automaton --run

# Show current status
automaton --status
```

## Tests

```sh
cargo test
```

28 tests covering agent tools, injection defense, skills parsing, config, wallet, self-modification, and audit logging.

## Performance

Compared to the original TypeScript implementation:

| Metric | Rust | TypeScript |
|--------|------|------------|
| Startup | <1ms | ~1,340ms |
| Memory | ~4.8MB | ~46.5MB |
| Binary | 3.4MB (release) | N/A (Node.js) |
| Source | 58 files, 15K lines | 46 files, 10K lines |

## License

Private.
