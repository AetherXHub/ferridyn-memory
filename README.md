# fmemory — Structured Memory CLI

Persistent, structured memory backed by [FerridynDB](https://github.com/AetherXHub/ferridyndb). Store and retrieve memories with typed attributes and secondary indexes, using Claude Haiku for schema inference, natural language parsing, and query resolution.

## Install

Requires Rust 2024 edition (stable 1.85+).

```bash
cargo install --git https://github.com/AetherXHub/ferridyn-memory
```

Also install and start the database server:

```bash
cargo install ferridyn-server --git https://github.com/AetherXHub/ferridyndb

mkdir -p ~/.local/share/ferridyn
ferridyn-server \
  --db ~/.local/share/ferridyn/memory.db \
  --socket ~/.local/share/ferridyn/server.sock &
```

Set `ANTHROPIC_API_KEY` for schema inference and natural language features (see [when it's required](#environment-variables)).

## Usage

### Store a memory

```bash
# Natural language — schema auto-inferred on first write
fmemory remember "Toby is a backend engineer, email toby@example.com"

# With explicit category
fmemory remember --category contacts "Toby is a backend engineer"

# Full control
fmemory remember --category contacts --key toby "backend engineer at Example Corp"
```

On first write to a new category, Haiku infers a typed schema (attributes like `name: STRING`, `email: STRING`) and creates secondary indexes automatically. Subsequent writes parse input against the existing schema.

Relative dates are resolved automatically — "meeting tomorrow at 3pm" becomes an absolute date.

### Retrieve memories

```bash
# Natural language query (index-optimized, synthesized answer)
fmemory recall --query "Toby's email"

# Exact lookup
fmemory recall --category contacts --key toby

# Category scan
fmemory recall --category contacts --limit 10

# Prompt mode (classifies intent -> remember or recall)
fmemory -p "what's Toby's email?"
fmemory -p "remember that staging is at staging.example.com"
```

NL queries (`--query` and `-p`) synthesize a natural language answer from retrieved data. With `--json`, the raw items are returned instead.

If a query returns no results, the CLI automatically broadens the search to scan the full category before giving up.

### Browse structure

```bash
# List all categories with schema info
fmemory discover

# Drill into a category (keys, schema, indexes)
fmemory discover --category contacts --limit 50
```

### Remove a memory

```bash
fmemory forget --category contacts --key toby
```

### Define a schema explicitly

```bash
fmemory define \
  --category contacts \
  --description "People and their contact info" \
  --attributes '[{"name":"name","type":"STRING","required":true},{"name":"email","type":"STRING","required":false}]' \
  --auto-index
```

Attribute types: `STRING`, `NUMBER`, `BOOLEAN`. The `--auto-index` flag creates a secondary index for each attribute. Explicitly defined schemas enforce validation on writes; auto-inferred schemas do not.

### View schema

```bash
# Single category (attributes + indexes)
fmemory schema --category contacts

# All schemas
fmemory schema
```

## CLI Reference

### Global flags

| Flag | Description |
|------|-------------|
| `--json` | Machine-readable JSON output to stdout (default: human-readable prose) |
| `-p, --prompt <text>` | Natural language prompt — classifies intent and routes to remember or recall. Requires `ANTHROPIC_API_KEY`. |

### Subcommands

#### `remember [--category CAT] [--key KEY] <input...>`

Store a memory. Input is positional (remaining args joined by space).

| Flag | Type | Required | Description |
|------|------|----------|-------------|
| `--category` | String | No | Target category. If omitted, Haiku infers one from input. |
| `--key` | String | No | Item key. If omitted, Haiku extracts one from the parsed document. |

Requires `ANTHROPIC_API_KEY` (always — for schema inference or document parsing).

#### `recall [--category CAT] [--key KEY] [--query Q] [--limit N]`

Retrieve memories. Provide `--category` (with optional `--key`) or `--query`, not both.

| Flag | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `--category` | String | No | — | Retrieve from this category |
| `--key` | String | No | — | Exact item lookup (requires `--category`) |
| `--query` | String | No | — | Natural language query. Requires `ANTHROPIC_API_KEY`. |
| `--limit` | usize | No | 20 | Maximum items returned |

In prose mode, NL queries produce a synthesized answer via Haiku. In `--json` mode, raw items are returned.

#### `discover [--category CAT] [--limit N]`

Browse memory structure. Does not require `ANTHROPIC_API_KEY`.

| Flag | Type | Required | Default | Description |
|------|------|----------|---------|-------------|
| `--category` | String | No | — | Drill into a category (shows keys, schema, indexes) |
| `--limit` | usize | No | 20 | Maximum items |

Without `--category`: lists all categories with description, attribute count, and index count.

#### `forget --category CAT --key KEY`

Remove a specific memory. Both flags are required. Does not require `ANTHROPIC_API_KEY`.

#### `define --category CAT --description DESC --attributes JSON [--auto-index]`

Explicitly create a category schema with typed attributes. All three main flags are required.

| Flag | Type | Required | Description |
|------|------|----------|-------------|
| `--category` | String | Yes | Category name |
| `--description` | String | Yes | Human-readable description |
| `--attributes` | String | Yes | JSON array: `[{"name":"...","type":"STRING","required":true}]` |
| `--auto-index` | bool | No | Create secondary indexes for all attributes |

Does not require `ANTHROPIC_API_KEY`.

#### `schema [--category CAT]`

View schema and index info. Without `--category`, lists all schemas. Does not require `ANTHROPIC_API_KEY`.

### Output conventions

- **Data** (items, schemas, JSON) goes to **stdout**
- **Status messages** ("Stored ...", "Forgot ...", "No memories found") go to **stderr**
- This separation lets you pipe JSON output while still seeing status messages

## Architecture

```
ferridyn-server (background daemon, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- fmemory CLI
```

The CLI connects to `ferridyn-server` via Unix socket. The `memories` table uses `category` as the partition key and `key` as the sort key.

### How schemas work

On first write to a new category, Claude Haiku (`claude-haiku-4-5`) infers a schema:
1. Analyzes the input to determine typed attributes and suggests secondary indexes
2. Creates the partition schema and indexes in the database
3. Parses the input into a structured document matching the schema

Subsequent writes parse input against the existing schema. Relative dates ("tomorrow", "next week") are resolved to absolute ISO 8601 dates.

### How NL queries work

Natural language queries go through a multi-step resolution:
1. Haiku resolves the query to one of: index lookup, partition scan with key prefix, or exact lookup
2. The resolved query executes against the database
3. If no results, falls back to scanning the full category
4. In prose mode, Haiku synthesizes a natural language answer from retrieved items

## Environment Variables

| Variable | Required | Purpose |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | For NL features | Schema inference, NL parsing, query resolution, answer synthesis. Not needed for `discover`, `forget`, `schema`, or `recall --category`. |
| `FERRIDYN_MEMORY_SOCKET` | No | Override server socket path (default: `~/.local/share/ferridyn/server.sock`) |

## Claude Code Plugin

For Claude Code integration (auto-retrieval hooks, skills, agent behaviors), see [ferridyn-memory-plugin](https://github.com/AetherXHub/ferridyn-memory-plugin).

## Development

```bash
cargo build                       # compile
cargo test                        # 30 tests
cargo clippy -- -D warnings       # lint
cargo fmt --check                 # format check
```
