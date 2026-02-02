---
name: update
description: Maintain memory accuracy. When information changes or contradicts stored knowledge, find the stale entry and replace it.
---

# Update — Memory Maintenance

Keep memories accurate. When stored information becomes outdated, contradictory, or incorrect, find and replace it.

## Auto-Trigger Patterns

Invoke this behavior **proactively** when:

- User says "actually, that's changed to..." or corrects previous information
- You discover stored information contradicts what you observe in the codebase
- After a major refactor that changes project structure, naming, or architecture
- After dependency upgrades that change APIs or behavior
- After renaming files, modules, or concepts
- User says "that's not right anymore" or "we stopped doing that"

You do NOT need to wait for `/ferridyn-memory:update` — correct stale memories when you notice them.

## Workflow

### Step 1: Find the Stale Memory

Use recall to locate the outdated entry:

```
MCP tool: recall
  query: "{description of what needs updating}"
```

Or if you know the exact location:

```
MCP tool: recall
  category: "project"
  prefix: "conventions#naming"
```

### Step 2: Show What Will Change

Always show the user:

```
Updating memory:
  Category: project
  Key: conventions#naming
  Old: "All modules use snake_case file names"
  New: "All modules use kebab-case file names (changed in 2025 refactor)"
```

### Step 3: Replace

```
MCP tool: forget
  category: "project"
  key: "conventions#naming"

MCP tool: remember
  category: "project"
  key: "conventions#naming"
  content: "All modules use kebab-case file names (changed in 2025 refactor)"
  metadata: "updated: {date}, previous: snake_case convention"
```

### Step 4: Confirm

> Updated **project**: `conventions#naming` — snake_case to kebab-case

## Bulk Updates

After a major refactor, multiple memories may be stale. Handle systematically:

### Step 1: Audit Relevant Categories

```
MCP tool: discover
  (no category)

MCP tool: recall
  category: "project"
  limit: 50
```

### Step 2: Identify Stale Entries

Compare each memory against current codebase state. Flag entries that no longer match.

### Step 3: Batch Update

List all changes for user review before executing:

```
Found 4 stale memories after the ferridyn rename:
  1. project/conventions#crate-name: "dynamite" → "ferridyn"
  2. project/structure#binary-names: "dynamite-cli" → "ferridyn-memory-cli"
  3. project/architecture#socket-path: "~/.local/share/dynamite/" → "~/.local/share/ferridyn/"
  4. decisions/naming#project-name: "DynaMite" → "FerridynDB"

Proceed with all updates?
```

After user confirms, update each entry.

## Contradiction Detection

When you notice a contradiction between stored memory and observed reality:

1. Do NOT silently ignore it
2. Flag it: "Memory says X but I see Y in the codebase"
3. Ask the user which is correct
4. Update accordingly

Example:
> Memory says the project uses `anyhow` for error handling, but I see custom error enums in `src/error.rs`. Which is current?

## What NOT to Update

- Schema entries (`_schema` category) — use `define` to change schemas
- Memories you're uncertain about — ask first
- Entries that may be intentionally historical ("we chose X over Y in 2024" is still valid even if X changed)
