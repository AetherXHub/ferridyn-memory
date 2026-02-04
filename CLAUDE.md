# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

FerridynDB Memory is a Claude Code plugin that gives Claude persistent, structured memory backed by the FerridynDB database server. It stores and retrieves memories via a CLI (`fmemory`), with automatic context retrieval before each prompt and learning extraction before conversation compaction.

## Why

Claude Code sessions are ephemeral — knowledge is lost when conversations end or compact. This plugin solves that by persisting structured memories (decisions, contacts, project knowledge, patterns) in a local FerridynDB database via the `ferridyn-server` daemon. Memories are organized by category with typed attributes, and the system uses Claude Haiku to automatically infer schemas (with typed attributes and secondary indexes), parse natural language input, and resolve queries to the right data.

## Build Commands

- `npm install` — install Node.js dev dependencies (tsup, typescript)
- `npm run build:scripts` — compile TypeScript hook scripts to `scripts/dist/`
- `npm run build:all` — build scripts + Rust binaries (full build)
- `cargo build` — compile the plugin (fmemory CLI)
- `cargo build --release` — release build (required for plugin deployment)
- `cargo test` — run all tests (25 tests)
- `cargo clippy -- -D warnings` — lint
- `cargo fmt --check` — check formatting

## How It Works

### Server-Only Architecture

The CLI connects to a running `ferridyn-server` via Unix socket. The server must be running.

```
ferridyn-server (background daemon, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- fmemory          (CLI: human use, hooks, Claude via Bash)
```

### Structured Memory with Native Schemas

Each memory category can have a native partition schema with typed attributes and secondary indexes:
- **Auto-inference** — On first write to a new category, Claude Haiku infers typed attributes (e.g., `name: STRING`, `email: STRING`) and suggests secondary indexes
- **NL parsing** — Natural language input like "Toby is a backend engineer" is parsed into structured attributes
- **Index-optimized queries** — Natural language queries like "Toby's email" are resolved using secondary indexes when available for fast attribute-based lookups
- **Structured data** — Items have typed attributes (name, email, role), not flat content strings. Keys are simple identifiers (`toby`), not hierarchical formats.

### CLI Commands

The `fmemory` CLI provides 6 commands plus a natural-language-first mode:

| Command | Purpose |
|---------|---------|
| `remember` | Store a memory — NL-first (auto-infers schema with typed attributes and indexes on first write) |
| `recall` | Retrieve memories by category+key, category scan, or NL query (index-optimized) |
| `discover` | Browse categories with schema descriptions, attribute counts, and index counts |
| `forget` | Remove a specific memory by category and key |
| `define` | Explicitly define or update a category's schema with typed attributes |
| `schema` | View schema for a category (attributes, indexes) |
| NL-first mode | `fmemory "natural language query"` — resolves to recall |

### Hooks (3)

- **UserPromptSubmit** (`memory-retrieval.ts`) — Before each prompt, discovers stored memories, selects relevant ones via Haiku, injects structured items as context. Also injects the **Memory Protocol** — behavioral guidance that tells the agent when to proactively commit, retrieve, and correct memories mid-conversation.
- **PreCompact** (`memory-commit.ts`) — Before conversation compaction, extracts key learnings from the transcript via Haiku and persists them using NL-first store format
- **Stop** (`memory-reflect.ts`) — When a session ends, reflects on the conversation and persists high-level learnings (decisions, patterns, preferences). Complementary to PreCompact — focuses on big-picture takeaways rather than granular facts.

### Skills (13)

#### Core Skills (user-invoked)

| Skill | Purpose |
|-------|---------|
| `setup` | Build binaries and scripts, start server, activate hooks |
| `remember` | Guidance on what/how to store memories |
| `recall` | Precise and natural language retrieval patterns |
| `forget` | Safe memory removal workflow |
| `browse` | Interactive memory exploration |
| `learn` | Deep codebase exploration that builds persistent project memory |
| `health` | Memory integrity diagnostics — schema coverage, index coverage, empty categories, issues |

#### Proactive Skills (agent auto-triggered + user-invokable)

| Skill | Auto-Trigger | Purpose |
|-------|-------------|---------|
| `teach` | "remember that...", "note that...", "from now on..." | Parse natural language into structured memory — user doesn't need to know categories or keys |
| `reflect` | After completing significant work | Extract and persist learnings — decisions, patterns, gotchas, preferences |
| `context` | Before starting complex work | Pull relevant memories; ask and store if expected knowledge is missing |
| `update` | When information changes or contradicts stored data | Find stale memories and replace them |
| `decide` | When a significant decision is made | Log decisions with rationale, alternatives, and constraints |
| `status` | Session start, "what do you know about..." | Quick overview of memory contents by category |

## Architecture

```
src/
  schema.rs    — SchemaManager: InferredSchema, ResolvedQuery, LLM prompts for schema inference, NL parsing, and query resolution
  llm.rs       — LlmClient trait + AnthropicClient (Claude Haiku) + MockLlmClient for tests
  backend.rs   — MemoryBackend: server-only (FerridynClient), schema/index creation methods
  lib.rs       — Shared: socket path resolution, table initialization, env var handling
  cli.rs       — fmemory binary: NL-first writes, index-optimized reads, prose output, --json flag
  error.rs     — Error types
```

```
skills/       — 13 SKILL.md files (setup, remember, recall, forget, browse, learn, health,
                teach, reflect, context, update, decide, status)
commands/     — 13 command .md files
hooks/        — hooks.json (UserPromptSubmit, PreCompact, Stop)
scripts/
  src/                    — TypeScript source (compiled by tsup)
    types.ts              — Shared type definitions
    config.ts             — Shared utilities: CLI runner, Haiku caller, JSON extraction
    memory-retrieval.ts   — UserPromptSubmit hook + memory protocol injection
    memory-commit.ts      — PreCompact hook (auto-save)
    memory-reflect.ts     — Stop hook (session reflection)
    memory-health.ts      — Diagnostics utility (standalone)
    memory-stats.ts       — Stats utility (standalone)
  dist/                   — Built output (.mjs files, produced by `npm run build:scripts`)
.claude-plugin/           — Plugin metadata (plugin.json, marketplace.json)
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
| `FERRIDYN_MEMORY_CLI` | No | Override `fmemory` binary path (used by hook scripts) |

## Development Process

1. **Build scripts** — `npm run build:scripts` must produce 5 `.mjs` files in `scripts/dist/`
2. **Build Rust** — `cargo build` must pass
3. **Test** — `cargo test` must pass (25 tests covering schema inference, LLM mocking, CLI command handlers, backend operations)
4. **Lint** — `cargo clippy -- -D warnings` must pass
5. **Format** — `cargo fmt --check` must pass
