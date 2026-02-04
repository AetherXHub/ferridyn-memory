# fmemory — Structured Memory CLI

Persistent, structured memory backed by [FerridynDB](https://github.com/AetherXHub/ferridyndb). Store and retrieve memories with typed attributes and secondary indexes, using Claude Haiku for schema inference, natural language parsing, and query resolution.

## Install

```bash
cargo install --git https://github.com/AetherXHub/ferridyn-memory
```

Also install and start the database server:

```bash
cargo install ferridyn-server --git https://github.com/AetherXHub/ferridyndb

# Start the server
mkdir -p ~/.local/share/ferridyn
ferridyn-server \
  --db ~/.local/share/ferridyn/memory.db \
  --socket ~/.local/share/ferridyn/server.sock &
```

Set `ANTHROPIC_API_KEY` for schema inference and natural language features.

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

### Retrieve memories

```bash
# Natural language query (index-optimized)
fmemory recall --query "Toby's email"

# Exact lookup
fmemory recall --category contacts --key toby

# Category scan
fmemory recall --category contacts

# Prompt mode (classifies intent → remember or recall)
fmemory -p "what's Toby's email?"
```

### Browse structure

```bash
# List all categories with schema info
fmemory discover

# Drill into a category
fmemory discover --category contacts
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

### View schema

```bash
fmemory schema --category contacts
```

## Commands

| Command | Purpose |
|---------|---------|
| `remember` | Store a memory (auto-infers schema on first write) |
| `recall` | Retrieve by category+key, category scan, or NL query |
| `discover` | Browse categories, schemas, and indexes |
| `forget` | Remove a specific memory |
| `define` | Explicitly define a category schema |
| `schema` | View schema and indexes for a category |
| `-p` | Natural language prompt (routes to remember or recall) |

All commands support `--json` for machine-readable output.

## Architecture

```
ferridyn-server (background daemon, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- fmemory CLI
```

The CLI connects to `ferridyn-server` via Unix socket. Each memory category can have a native partition schema with typed attributes and secondary indexes. On first write to a new category, Claude Haiku infers the schema automatically.

## Configuration

| Variable | Required | Purpose |
|----------|----------|---------|
| `ANTHROPIC_API_KEY` | Yes | Claude Haiku for schema inference, NL parsing, query resolution |
| `FERRIDYN_MEMORY_SOCKET` | No | Override server socket path |

## Claude Code Plugin

For Claude Code integration (auto-retrieval hooks, skills, agent behaviors), see [ferridyn-memory-plugin](https://github.com/AetherXHub/ferridyn-memory-plugin).

## Development

```bash
cargo build                       # compile
cargo test                        # run tests (30 tests)
cargo clippy -- -D warnings       # lint
cargo fmt --check                 # check formatting
```
