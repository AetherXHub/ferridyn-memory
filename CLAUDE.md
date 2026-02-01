# CLAUDE.md

## Project

DynaMite Memory is a Claude Code plugin providing agentic memory backed by the DynaMite embedded database. It stores and retrieves memories via an MCP server, with automatic context retrieval and learning hooks.

## Build Commands

- `cargo build` — compile the plugin (dynamite-memory MCP server + dynamite-memory-cli)
- `cargo build --release` — release build
- `cargo test` — run all tests
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — check formatting

## Dependencies

The plugin depends on `dynamite-core` and `dynamite-server` from the [dynamitedb](https://github.com/AetherXHub/dynamitedb) repository via git dependencies.

For local development against a local dynamitedb checkout, uncomment the `[patch]` section in `.cargo/config.toml`.

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
