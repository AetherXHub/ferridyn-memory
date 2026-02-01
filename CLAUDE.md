# CLAUDE.md

## Project

FerridynDB Memory is a Claude Code plugin providing agentic memory backed by the FerridynDB embedded database. It stores and retrieves memories via an MCP server, with automatic context retrieval and learning hooks.

## Build Commands

- `cargo build` — compile the plugin (ferridyn-memory MCP server + ferridyn-memory-cli)
- `cargo build --release` — release build
- `cargo test` — run all tests
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — check formatting

## Dependencies

The plugin depends on `ferridyn-core` and `ferridyn-server` from the [ferridyndb](https://github.com/AetherXHub/ferridyndb) repository via git dependencies.

For local development against a local ferridyndb checkout, uncomment the `[patch]` section in `.cargo/config.toml`.

## Architecture

- `src/server.rs` — MCP server with remember/recall/discover/forget/define tools
- `src/schema.rs` — Schema-aware memory system with LLM inference
- `src/llm.rs` — Anthropic API client for schema inference and NL recall
- `src/backend.rs` — MemoryBackend abstraction (server mode vs direct mode)
- `src/cli.rs` — CLI binary for hook scripts
- `src/main.rs` — MCP server binary entry point

## Plugin Structure

- `skills/` — 6 skill definitions (setup, remember, recall, forget, browse, learn)
- `commands/` — 6 command definitions
- `hooks/hooks.json` — UserPromptSubmit and PreCompact hooks
- `scripts/` — Hook implementation scripts (Node.js)
- `.claude-plugin/` — Plugin metadata
