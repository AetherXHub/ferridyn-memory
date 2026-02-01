# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

FerridynDB Memory is a Claude Code plugin that gives Claude persistent, schema-aware memory backed by the FerridynDB embedded database. It stores and retrieves memories via an MCP server, with automatic context retrieval before each prompt and learning extraction before conversation compaction.

## Why

Claude Code sessions are ephemeral — knowledge is lost when conversations end or compact. This plugin solves that by persisting structured memories (decisions, contacts, project knowledge, patterns) in a local FerridynDB database. Memories are organized by category with hierarchical sort keys, and the system uses Claude Haiku to automatically infer schemas, validate keys, and resolve natural language queries to the right data.

## Build Commands

- `cargo build` — compile the plugin (ferridyn-memory MCP server + ferridyn-memory-cli)
- `cargo build --release` — release build (required for plugin deployment)
- `cargo test` — run all tests (52 tests)
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — check formatting

## How It Works

### Two Operational Modes

The MCP server and CLI both try to connect to a running `ferridyn-server` via Unix socket first. If no server is available, they fall back to opening the database file directly (exclusive file lock).

```
ferridyn-server (background daemon, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- ferridyn-memory     (MCP server, provides tools to Claude)
    +-- ferridyn-memory-cli (used by hook scripts for read/write)
```

### Schema-Aware Memory

Each memory category can have a schema defining its sort key format:
- **Auto-inference** — On first write to a new category, Claude Haiku infers the schema from the data
- **Validation** — Subsequent writes are checked against the schema's expected key format
- **NL resolution** — Natural language queries like "Toby's email" are resolved to `category=people, prefix=toby` using schemas as context

Schemas are stored in a `_schema` meta-category within the same database.

### MCP Tools (5)

| Tool | Purpose |
|------|---------|
| `remember` | Store a memory (auto-infers schema on first write to a category) |
| `recall` | Retrieve memories by category+prefix or natural language query |
| `discover` | Browse categories with schema descriptions, drill into key prefixes |
| `forget` | Remove a specific memory by category and key |
| `define` | Explicitly define or update a category's key schema |

### Hooks (2)

- **UserPromptSubmit** (`memory-retrieval.mjs`) — Before each prompt, discovers stored memories, selects relevant ones via Haiku, injects as context
- **PreCompact** (`memory-commit.mjs`) — Before conversation compaction, extracts key learnings from the transcript via Haiku and persists them

### Skills (6)

| Skill | Purpose |
|-------|---------|
| `setup` | Build binaries, start server, activate MCP tools and hooks |
| `remember` | Guidance on what/how to store memories |
| `recall` | Precise and natural language retrieval patterns |
| `forget` | Safe memory removal workflow |
| `browse` | Interactive memory exploration |
| `learn` | Deep codebase exploration that builds persistent project memory |

## Architecture

```
src/
  server.rs    — MCP server (rmcp 0.14): tool handlers for remember/recall/discover/forget/define
  schema.rs    — Schema system: CategorySchema, SchemaStore, inference, NL query resolution, validation
  llm.rs       — LlmClient trait + AnthropicClient (Claude Haiku) + MockLlmClient for tests
  backend.rs   — MemoryBackend enum: Direct(FerridynDB) | Server(FerridynClient) — unified async API
  lib.rs       — Shared: socket/DB path resolution, table initialization, env var handling
  cli.rs       — ferridyn-memory-cli binary (clap): discover/recall/remember/forget/define/schema
  main.rs      — ferridyn-memory binary: MCP server entry point
  error.rs     — Error types
```

```
skills/       — 6 SKILL.md files (setup, remember, recall, forget, browse, learn)
commands/     — 6 command .md files
hooks/        — hooks.json (UserPromptSubmit, PreCompact)
scripts/      — Hook implementations (Node.js, zero npm deps)
  config.mjs            — Shared utilities: CLI runner, Haiku caller, JSON extraction
  memory-retrieval.mjs  — Auto-retrieval hook
  memory-commit.mjs     — Auto-save hook
.claude-plugin/         — Plugin metadata (plugin.json, marketplace.json)
```

## Dependencies

This crate depends on `ferridyn-core` and `ferridyn-server` from the [ferridyndb](https://github.com/AetherXHub/ferridyndb) repository via git dependencies. For local development against a local ferridyndb checkout, uncomment the `[patch]` section in `.cargo/config.toml`.

### Key crates
- `ferridyn-core` / `ferridyn-server` — Database engine and client
- `rmcp` — MCP server framework (Model Context Protocol)
- `tokio` — Async runtime
- `reqwest` — HTTP client for Anthropic API calls
- `clap` — CLI argument parsing
- `schemars` — JSON Schema generation for MCP tool parameters
- `regex` — Sort key format validation
- `indexmap` — Ordered maps for schema segments

## Environment Variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | Yes | Claude Haiku for schema inference and NL recall |
| `FERRIDYN_MEMORY_SOCKET` | No | Override server socket path (default: `~/.local/share/ferridyn/server.sock`) |
| `FERRIDYN_MEMORY_DB` | No | Override database file path (default: `~/.local/share/ferridyn/memory.db`) |
| `FERRIDYN_MEMORY_CLI` | No | Override CLI binary path (used by hook scripts) |

## Development Process

1. **Build** — `cargo build` must pass
2. **Test** — `cargo test` must pass (52 tests covering schema validation, LLM mocking, MCP tool handlers, backend operations)
3. **Lint** — `cargo clippy -- -D warnings` must pass
4. **Format** — `cargo fmt --check` must pass
