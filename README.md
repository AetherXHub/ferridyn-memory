# FerridynDB Memory Plugin for Claude Code

Self-contained Claude Code plugin that provides:

1. **MCP server** — `remember`, `recall`, `discover`, `forget` tools available directly in Claude
2. **Auto-retrieval hook** (UserPromptSubmit) — automatically injects relevant memories into context before Claude processes each prompt
3. **Auto-save hook** (PreCompact) — extracts and persists important learnings before conversation compaction
4. **Setup skill** — `/ferridyn-memory:setup` bootstraps the entire system

## Install

```bash
# Add the plugin to Claude Code
/plugin marketplace add AetherXHub/ferridyn-memory
```

After installing, run `/ferridyn-memory:setup` to build binaries, start the server, and verify everything works.

## Setup

Run `/ferridyn-memory:setup` in Claude Code. It will:

1. Build the release binaries
2. Start the FerridynDB server
3. Verify the CLI and MCP tools work
4. Test a round-trip memory store/recall/forget

## Architecture

```
ferridyn-server (background, owns DB file)
    ^ Unix socket (~/.local/share/ferridyn/server.sock)
    |
    +-- ferridyn-memory (MCP server, provides tools to Claude)
    +-- ferridyn-memory-cli (used by hooks for read/write)
```

## Hooks

- **memory-retrieval.mjs** (UserPromptSubmit): Discovers stored memories, selects relevant ones (using Claude Haiku if ANTHROPIC_API_KEY is set, or fetches all as fallback), injects as `additionalContext`
- **memory-commit.mjs** (PreCompact): Reads recent transcript, uses Claude Haiku to extract key learnings, stores them as memories

## Configuration

- `FERRIDYN_MEMORY_CLI` — override CLI binary path
- `FERRIDYN_MEMORY_SOCKET` — override server socket path
- `ANTHROPIC_API_KEY` — enables intelligent memory selection/extraction via Claude Haiku

## Plugin Structure

```
(repo root)
  .claude-plugin/plugin.json   — plugin metadata
  .mcp.json                    — MCP server declaration
  hooks/hooks.json             — hook configuration
  scripts/
    config.mjs                 — shared utilities
    memory-retrieval.mjs       — auto-retrieval hook
    memory-commit.mjs          — auto-save hook
  skills/setup/SKILL.md        — /ferridyn-memory:setup
```
