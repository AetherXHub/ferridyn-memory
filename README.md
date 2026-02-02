# FerridynDB Memory Plugin for Claude Code

Persistent, schema-aware memory for Claude Code backed by [FerridynDB](https://github.com/AetherXHub/ferridyndb). Memories survive across sessions and conversation compactions — decisions, contacts, project knowledge, patterns, and preferences are stored locally and recalled automatically.

## What It Does

- **CLI interface** — `fmemory` command with 6 subcommands plus natural-language-first mode available to Claude via Bash
- **3 hooks** — auto-retrieve context before each prompt, save learnings before compaction, reflect on sessions at exit
- **13 skills** — proactive agent behaviors (teach, reflect, context, update, decide, status) plus core workflows (setup, remember, recall, forget, browse, learn, health)
- **Schema-aware** — Claude Haiku auto-infers category schemas on first write, validates keys, and resolves natural language queries

## Install

```bash
# Add the plugin to Claude Code
/plugin marketplace add AetherXHub/ferridyn-memory
```

After installing, run `/ferridyn-memory:setup` to build binaries, start the server, and activate everything.

## Setup

`/ferridyn-memory:setup` handles the full bootstrap:

1. Check prerequisites (Rust toolchain, Node.js, `ANTHROPIC_API_KEY`)
2. Install npm dependencies and build TypeScript hook scripts (`tsup`)
3. Build Rust release binaries and install `ferridyn-server`
4. Create data directory and start the server daemon
5. Verify round-trip memory storage

Restart Claude Code after setup for hooks to take effect.

## Architecture

```
ferridyn-server (background daemon, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- fmemory          (CLI: human use, hooks, Claude via Bash)
```

The CLI tries the server socket first. If unavailable, it falls back to opening the database file directly (exclusive file lock).

### Schema-Aware Memory

Each category can have a schema defining its sort key format:

- **Auto-inference** — On first write to a new category, Claude Haiku infers the schema
- **Validation** — Subsequent writes are checked against the expected key format
- **NL resolution** — Natural language queries like "Toby's email" resolve to `category=people, prefix=toby`

## CLI Commands

The `fmemory` CLI provides 6 commands plus a natural-language-first mode:

| Command | Purpose |
|---------|---------|
| `remember` | Store a memory (auto-infers schema on first write) |
| `recall` | Retrieve memories by category+prefix or natural language query |
| `discover` | Browse categories with schema descriptions, drill into key prefixes |
| `forget` | Remove a specific memory by category and key |
| `define` | Explicitly define or update a category's key schema |
| `schema` | View schema for a category |
| NL-first mode | `fmemory "natural language query"` — resolves to recall or other command |

## Hooks

| Hook | Event | Script | Purpose |
|------|-------|--------|---------|
| Auto-retrieval | UserPromptSubmit | `memory-retrieval.ts` | Select and inject relevant memories before each prompt. Injects the **Memory Protocol** — behavioral guidance for proactive memory use. |
| Auto-save | PreCompact | `memory-commit.ts` | Extract key learnings from the transcript before context compaction |
| Session reflect | Stop | `memory-reflect.ts` | Reflect on the full session and persist high-level decisions, patterns, and preferences |

## Skills

### Core (user-invoked)

| Skill | Purpose |
|-------|---------|
| `/ferridyn-memory:setup` | Build, start server, activate hooks |
| `/ferridyn-memory:remember` | Guidance on what and how to store |
| `/ferridyn-memory:recall` | Precise and natural language retrieval |
| `/ferridyn-memory:forget` | Safe memory removal workflow |
| `/ferridyn-memory:browse` | Interactive memory exploration |
| `/ferridyn-memory:learn` | Deep codebase exploration that builds persistent project memory |
| `/ferridyn-memory:health` | Memory integrity diagnostics |

### Proactive (agent auto-triggered + user-invokable)

| Skill | Auto-Trigger | Purpose |
|-------|-------------|---------|
| `/ferridyn-memory:teach` | "remember that...", "note that...", "from now on..." | Parse natural language into structured memory |
| `/ferridyn-memory:reflect` | After completing significant work | Extract decisions, patterns, gotchas |
| `/ferridyn-memory:context` | Before starting complex work | Pull relevant memories; ask and store if missing |
| `/ferridyn-memory:update` | When stored info contradicts current knowledge | Find and replace stale memories |
| `/ferridyn-memory:decide` | When a significant decision is made | Log decision with rationale and alternatives |
| `/ferridyn-memory:status` | Session start | Quick overview of memory contents |

## Configuration

| Variable | Required | Purpose |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | Yes | Claude Haiku for schema inference, NL recall, and hook scripts |
| `FERRIDYN_MEMORY_SOCKET` | No | Override server socket path (default: `~/.local/share/ferridyn/server.sock`) |
| `FERRIDYN_MEMORY_DB` | No | Override database file path (default: `~/.local/share/ferridyn/memory.db`) |
| `FERRIDYN_MEMORY_CLI` | No | Override `fmemory` binary path (used by hook scripts) |

## Project Structure

```
src/
  schema.rs           — Schema system: inference, validation, NL resolution
  llm.rs              — LLM client (Claude Haiku) + mock for tests
  backend.rs          — MemoryBackend: Direct(FerridynDB) | Server(FerridynClient)
  cli.rs              — fmemory binary: CLI with NL-first mode and --json output
  lib.rs              — Shared utilities
scripts/
  src/                — TypeScript source (compiled by tsup)
    config.ts         — Shared: CLI runner, Haiku caller, JSON extraction
    types.ts          — Shared type definitions
    memory-retrieval.ts   — UserPromptSubmit hook
    memory-commit.ts      — PreCompact hook
    memory-reflect.ts     — Stop hook
    memory-health.ts      — Diagnostics utility
    memory-stats.ts       — Stats utility
  dist/               — Built output (.mjs), produced by npm run build:scripts
skills/               — 13 skill definitions (SKILL.md files)
commands/             — 13 command definitions (.md files)
hooks/hooks.json      — Hook configuration
.claude-plugin/       — Plugin metadata
```

## Development

```bash
npm install                       # install tsup + typescript
npm run build:scripts             # compile TypeScript hooks to scripts/dist/
cargo build                       # compile Rust binaries
cargo test                        # run tests (41 tests)
cargo clippy -- -D warnings       # lint
cargo fmt --check                 # check formatting
```
