# DynaMite Memory

An agentic memory system for Claude Code, backed by DynaMite.

## What it provides

- **MCP server binary** (`dynamite-memory`) exposing 4 tools: remember, recall, discover, forget
- **CLI binary** (`dynamite-memory-cli`) with the same 4 subcommands
- **Shared library** with MemoryBackend abstraction (Direct DB or Server client)

## MCP Tools

- `remember(category, key, content, metadata?)` — store a memory
- `recall(category, prefix?, limit?)` — retrieve memories by category
- `discover(category?, limit?)` — browse categories and sort key prefixes
- `forget(category, key)` — delete a memory

## CLI Usage

```bash
dynamite-memory-cli discover
dynamite-memory-cli discover --category rust
dynamite-memory-cli recall --category rust --prefix ownership --limit 10
dynamite-memory-cli remember --category rust --key "ownership#borrowing" --content "References allow borrowing"
dynamite-memory-cli forget --category rust --key "ownership#borrowing"
```

## Backend modes

- **Server mode**: Connects to `dynamite-server` via Unix socket (preferred — allows concurrent access)
- **Direct mode**: Opens database file directly with exclusive flock (fallback when server not running)

## Environment variables

- `DYNAMITE_MEMORY_SOCKET` — socket path (default: `~/.local/share/dynamite/server.sock`)
- `DYNAMITE_MEMORY_DB` — database path for direct mode (default: `~/.local/share/dynamite/memory.db`)

## Data model

Memories stored in a `memories` table with:

- **Partition key**: `category` (String) — semantic category like "rust-patterns", "project-context"
- **Sort key**: `key` (String) — hierarchical identifier using `#` separator like "ownership#borrowing"
- **Attributes**: `content` (the memory text), optional `metadata`
