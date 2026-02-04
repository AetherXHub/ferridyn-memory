# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

fmemory is a CLI tool for persistent, structured memory backed by the FerridynDB database server. It stores and retrieves memories with typed attributes and secondary indexes, using Claude Haiku (`claude-haiku-4-5`) for schema inference, natural language parsing, and query resolution.

The companion Claude Code plugin lives at [ferridyn-memory-plugin](https://github.com/AetherXHub/ferridyn-memory-plugin).

## Build Commands

- `cargo build` — compile the fmemory binary
- `cargo build --release` — release build
- `cargo test` — run all tests (30 tests)
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — check formatting

Requires Rust 2024 edition (stable 1.85+).

## How It Works

### Server-Only Architecture

The CLI connects to a running `ferridyn-server` via Unix socket. If the socket doesn't exist, the CLI exits with: "ferridyn-server socket not found at PATH. Start the server with: ferridyn-server".

```
ferridyn-server (background daemon, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- fmemory CLI
```

The `memories` table uses `category` as partition key and `key` as sort key.

### LLM Integration

Six LLM prompt constants drive the system. All use `claude-haiku-4-5` with 2048 max tokens.

| Prompt | Location | Purpose |
|--------|----------|---------|
| `INFER_CATEGORY_PROMPT` | cli.rs | Given NL input + existing categories, infer which category it belongs to |
| `INFER_SCHEMA_PROMPT` | schema.rs | Given a category name + first input, infer typed attributes (STRING/NUMBER/BOOLEAN) and suggest indexes |
| `PARSE_DOCUMENT_PROMPT` | schema.rs | Parse NL input into structured JSON matching a schema. Resolves relative dates to absolute ISO 8601. |
| `RESOLVE_QUERY_PROMPT` | schema.rs | Resolve NL query to one of: index_lookup, partition_scan (with optional key prefix), or exact_lookup |
| `CLASSIFY_INTENT_PROMPT` | schema.rs | Classify NL input as "remember" or "recall" intent. Defaults to "remember" if ambiguous. |
| `ANSWER_QUERY_PROMPT` | schema.rs | Synthesize a natural language answer from retrieved items. Returns "NO_RELEVANT_DATA" sentinel if nothing matches. |

### Remember Flow

The `remember` command follows different paths based on state:

1. **No `--category`**: LLM call to `infer_category()` using existing schemas as context
2. **No schema exists for category**: LLM call to `infer_schema()` → creates schema + indexes → LLM call to `parse_to_document()`
3. **Schema exists**: LLM call to `parse_to_document()` against existing schema
4. **Schema creation fails**: Falls back to storing as `{"content": "raw text"}` with key `"unknown"`

Key resolution priority: `--key` flag > LLM-parsed `"key"` field > `"unknown"`.

Auto-inferred schemas are created with `validate: false` (permissive). Explicitly defined schemas (`define` command) use `validate: true` (strict).

### Query Resolution

NL queries (`--query` or `-p` in recall mode) go through:

1. `resolve_query()` — LLM classifies the query into one of three `ResolvedQuery` variants
2. Execute the resolved query against the backend
3. If no results, `execute_with_fallback()` broadens to a full partition scan
4. In prose mode, `answer_query()` synthesizes a natural language response from the items

### CLI Commands

| Command | Purpose | Requires API key |
|---------|---------|-----------------|
| `remember` | Store a memory (auto-infers schema on first write) | Yes (always) |
| `recall --category` | Retrieve by category, optional `--key` | No |
| `recall --query` | NL query with index-optimized resolution | Yes |
| `discover` | Browse categories, schemas, and indexes | No |
| `forget` | Remove a specific memory (`--category` and `--key` required) | No |
| `define` | Explicitly create a category schema with typed attributes | No |
| `schema` | View schema and indexes (single category or all) | No |
| `-p` prompt | NL prompt — classifies intent, routes to remember or recall | Yes |

Global flags: `--json` (machine-readable JSON to stdout), `-p`/`--prompt` (NL mode).

### Output Conventions

- **Data** (items, schemas, JSON) → stdout
- **Status messages** ("Stored ...", "Forgot ...", "No memories found") → stderr
- Prose mode: attribute names are capitalized, null values are hidden, non-string values print as JSON
- `--json` on recall skips LLM answer synthesis and returns raw items
- `--json` has no effect on `forget` or `define` (status-only commands)

## Architecture

```
src/
  cli.rs       — fmemory binary: Clap parsing, subcommands, -p prompt mode,
                 category inference, output formatting, fallback logic
  schema.rs    — SchemaManager, InferredSchema, ResolvedQuery, NlIntent,
                 6 LLM prompt constants, all inference/parsing/resolution functions
  llm.rs       — LlmClient trait + AnthropicClient (claude-haiku-4-5, 2048 tokens)
                 + MockLlmClient for tests. LlmError: MissingApiKey, Http, Parse, EmptyResponse.
  backend.rs   — MemoryBackend enum: Server(FerridynClient) + Direct(FerridynDB, #[cfg(test)] only)
                 16 methods: CRUD + schema/index operations
  lib.rs       — TABLE_NAME ("memories"), resolve_socket_path(), ensure_memories_table_via_server(),
                 re-exports: AttributeDefInput, AttributeInfo, IndexInfo, PartitionSchemaInfo, QueryResult
  error.rs     — MemoryError: Server, ServerUnavailable, Schema, Index, InvalidParams, Internal
```

### Key Types

**InferredSchema** (schema.rs): `description: String`, `attributes: Vec<AttributeDef>`, `suggested_indexes: Vec<String>`. AttributeDef has `name`, `attr_type` (STRING/NUMBER/BOOLEAN), `required`.

**ResolvedQuery** (schema.rs): `IndexLookup { category, index_name, key_value }` | `PartitionScan { category, key_prefix: Option }` | `ExactLookup { category, key }`.

**NlIntent** (schema.rs): `Remember { content }` (verb stripped) | `Recall { query }`.

### SchemaManager Methods

- `has_schema(category)` — bool check via describe_schema
- `get_schema(category)` — returns `Option<PartitionSchemaInfo>`
- `list_schemas()` — all partition schemas
- `create_schema_with_indexes(category, inferred, validate)` — creates schema + indexes named `{category}_{attribute}`
- `list_indexes()` — all secondary indexes
- `find_index(category, attribute)` — lookup by naming convention `{category}_{attribute}`

### MemoryBackend Methods

CRUD: `put_item`, `get_item`, `query` (with optional prefix + limit), `delete_item`, `list_partition_keys`, `list_sort_key_prefixes`.

Schema: `create_schema`, `describe_schema`, `list_schemas`, `drop_schema`.

Index: `create_index`, `list_indexes`, `describe_index`, `drop_index`, `query_index`.

## Dependencies

This crate depends on `ferridyn-server` from [ferridyndb](https://github.com/AetherXHub/ferridyndb) via git dependency. `ferridyn-core` is dev-only (used in tests). For local development against a local ferridyndb checkout, uncomment the `[patch]` section in `.cargo/config.toml`.

### Key crates
- `ferridyn-server` — Database server client
- `ferridyn-core` — Database engine (dev-only, for tests)
- `tokio` — Async runtime
- `reqwest` — HTTP client for Anthropic API calls
- `clap` — CLI argument parsing
- `chrono` — Date/time resolution (injected into LLM prompts)

## Environment Variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | For NL features | Claude Haiku for schema inference, NL parsing, query resolution, answer synthesis. Not needed for discover, forget, schema, or recall by category/key. |
| `FERRIDYN_MEMORY_SOCKET` | No | Override server socket path (default: `~/.local/share/ferridyn/server.sock`) |

## Development Process

1. **Build** — `cargo build` must pass
2. **Test** — `cargo test` must pass (30 tests covering schema inference, NL parsing, intent classification, query resolution, answer synthesis, backend operations)
3. **Lint** — `cargo clippy -- -D warnings` must pass
4. **Format** — `cargo fmt --check` must pass
