# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**fmemory** is a Rust CLI for persistent, structured memory storage backed by FerridynDB. It uses Claude Haiku (`claude-haiku-4-5`) for natural language parsing, query resolution, and answer synthesis. The binary is `fmemory`, the library crate is `ferridyn_memory`.

## Build & Development Commands

```bash
cargo build                    # Compile
cargo test                     # Run all 55 tests (no server needed; tests use direct DB)
cargo test <test_name>         # Run a single test by name
cargo test --lib schema        # Run tests in a specific module
cargo clippy -- -D warnings    # Lint
cargo fmt --check              # Check formatting
cargo fmt                      # Auto-format
```

**Environment variables:**
- `ANTHROPIC_API_KEY` — required at runtime for all NL features (not needed for tests; tests use `MockLlmClient`)
- `FERRIDYN_MEMORY_SOCKET` — override Unix socket path (default: `~/.local/share/ferridyn/server.sock`)

**Runtime prerequisite:** A running `ferridyn-server` daemon listening on the socket path. Tests bypass this via `MemoryBackend::Direct`.

## Architecture

```
cli.rs (binary entry point, command routing)
  ├── schema.rs  (core: predefined schemas, LLM prompts, query resolution)
  ├── backend.rs (MemoryBackend enum: Server | Direct)
  ├── llm.rs     (LlmClient trait, AnthropicClient, MockLlmClient)
  ├── ttl.rs     (client-side TTL: parse, compute, filter expired)
  └── error.rs   (MemoryError enum)
lib.rs (public API, re-exports, socket/DB path resolution)
```

**Key design patterns:**

- **`MemoryBackend` enum** (`backend.rs`) — `Server(FerridynClient)` for production (async, Unix socket) and `Direct(FerridynDB)` for tests (in-process). All backend methods are async. This is the single abstraction layer over FerridynDB.
- **`LlmClient` trait** (`llm.rs`) — `AnthropicClient` for production, `MockLlmClient` (FIFO queue) for tests. All LLM-dependent functions in `schema.rs` accept `&dyn LlmClient`.
- **9 predefined categories** (`schema.rs`, `PREDEFINED_SCHEMAS` constant) — compile-time schema definitions (project, decisions, contacts, preferences, issues, tools, events, notes, scratchpad). Each defines typed attributes and suggested secondary indexes.
- **Client-side TTL** (`ttl.rs`) — FerridynDB has no native TTL. Expiry is handled by `expires_at` RFC 3339 timestamps and `filter_expired()` at query time. `scratchpad` gets 24h default TTL; `events` auto-compute TTL from `date` attribute.
- **Intent classification** — The `-p/--prompt` flag uses `classify_intent()` to route input to either `remember` (store) or `recall` (retrieve) flow.
- **Fallback broadening** — If a targeted query (index lookup) returns nothing, `execute_with_fallback()` retries with a full partition scan.

**Data model:** Single table `memories` with partition key `category` (String) and sort key `key` (String). Items have category-specific typed attributes plus auto-injected `created_at` and optional `expires_at`.

## Module Responsibilities

- **`schema.rs`** (1,432 lines, 26 tests) — the core module. Contains all LLM system prompts, `SchemaManager` (wraps backend for schema/index ops), `ResolvedQuery` enum (IndexLookup/PartitionScan/ExactLookup), and all LLM-powered functions: `parse_to_document`, `parse_to_document_with_category`, `resolve_query`, `classify_intent`, `answer_query`.
- **`cli.rs`** (1,129 lines) — clap-based CLI with subcommands: init, remember, recall, forget, promote, prune, discover, define, schema. Contains `auto_init()` for first-use schema creation and all output formatting.
- **`backend.rs`** (559 lines, 7 tests) — CRUD, query, schema, and index operations on `MemoryBackend`. Error mapping from ferridyn_core/ferridyn_server errors to `MemoryError`.
- **`ttl.rs`** (226 lines, 13 tests) — TTL parsing (`24h`, `7d`, `2w`), expiry computation, client-side filtering, date-based auto-TTL for events.
- **`llm.rs`** (273 lines, 3 tests) — Anthropic API client (posts to `/v1/messages`, model `claude-haiku-4-5`, max 2048 tokens).

## Testing Patterns

- Tests use `MemoryBackend::Direct` with `tempfile` directories — no server needed.
- LLM-dependent tests use `MockLlmClient` with pre-programmed FIFO responses.
- `strip_markdown_fences()` handles LLM responses wrapped in ```json fences.
- `unsafe { std::env::remove_var(...) }` is used in one test for env var validation — this is intentional and noted with a safety comment.

## Dependencies

- `ferridyn-server` and `ferridyn-core` (dev) are git dependencies from `github.com/AetherXHub/ferridyndb`
- Rust edition 2024, requires stable 1.85+
