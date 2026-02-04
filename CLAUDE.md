# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

fmemory is a CLI tool for persistent, structured memory backed by the FerridynDB database server. It stores and retrieves memories with typed attributes and secondary indexes, using Claude Haiku for schema inference, natural language parsing, and query resolution.

The companion Claude Code plugin lives at [ferridyn-memory-plugin](https://github.com/AetherXHub/ferridyn-memory-plugin).

## Build Commands

- `cargo build` — compile the fmemory binary
- `cargo build --release` — release build
- `cargo test` — run all tests (30 tests)
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — check formatting

## How It Works

### Server-Only Architecture

The CLI connects to a running `ferridyn-server` via Unix socket. The server must be running.

```
ferridyn-server (background daemon, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- fmemory CLI
```

### Structured Memory with Native Schemas

Each memory category can have a native partition schema with typed attributes and secondary indexes:
- **Auto-inference** — On first write to a new category, Claude Haiku infers typed attributes (e.g., `name: STRING`, `email: STRING`) and suggests secondary indexes
- **NL parsing** — Natural language input like "Toby is a backend engineer" is parsed into structured attributes
- **Index-optimized queries** — Natural language queries like "Toby's email" are resolved using secondary indexes when available for fast attribute-based lookups
- **Structured data** — Items have typed attributes (name, email, role), not flat content strings. Keys are simple identifiers (`toby`), not hierarchical formats.

### CLI Commands

| Command | Purpose |
|---------|---------|
| `remember` | Store a memory (auto-infers schema with typed attributes and indexes on first write) |
| `recall` | Retrieve memories by category+key, category scan, or NL query (index-optimized) |
| `discover` | Browse categories with schema descriptions, attribute counts, and index counts |
| `forget` | Remove a specific memory by category and key |
| `define` | Explicitly define or update a category's schema with typed attributes |
| `schema` | View schema for a category (attributes, indexes) |
| `-p` prompt | `fmemory -p "natural language"` — classifies intent and routes to remember or recall |

## Architecture

```
src/
  schema.rs    — SchemaManager: InferredSchema, ResolvedQuery, LLM prompts for schema inference, NL parsing, and query resolution
  llm.rs       — LlmClient trait + AnthropicClient (Claude Haiku) + MockLlmClient for tests
  backend.rs   — MemoryBackend: server-only (FerridynClient), schema/index creation methods
  lib.rs       — Shared: socket path resolution, table initialization, env var handling
  cli.rs       — fmemory binary: -p prompt mode, subcommands, index-optimized reads, prose output, --json flag
  error.rs     — Error types
```

## Dependencies

This crate depends on `ferridyn-server` from the [ferridyndb](https://github.com/AetherXHub/ferridyndb) repository via git dependency. `ferridyn-core` is a dev-only dependency (used in tests). For local development against a local ferridyndb checkout, uncomment the `[patch]` section in `.cargo/config.toml`.

### Key crates
- `ferridyn-server` — Database server client
- `ferridyn-core` — Database engine (dev-only, for tests)
- `tokio` — Async runtime
- `reqwest` — HTTP client for Anthropic API calls
- `clap` — CLI argument parsing

## Environment Variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | Yes | Claude Haiku for schema inference, NL parsing, and query resolution |
| `FERRIDYN_MEMORY_SOCKET` | No | Override server socket path (default: `~/.local/share/ferridyn/server.sock`) |

## Development Process

1. **Build** — `cargo build` must pass
2. **Test** — `cargo test` must pass (30 tests covering schema inference, LLM mocking, CLI command handlers, backend operations)
3. **Lint** — `cargo clippy -- -D warnings` must pass
4. **Format** — `cargo fmt --check` must pass
