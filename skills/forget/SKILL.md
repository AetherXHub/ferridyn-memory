---
name: forget
description: Remove a specific memory from the DynaMite memory system. Use when information is outdated, incorrect, or explicitly requested to be removed.
---

# Forget — Remove a Memory

Use the MCP `forget` tool to remove specific memories.

## When to Remove

- Outdated information (old email, changed role, deprecated pattern)
- User explicitly asks to remove something
- Corrected data (delete the wrong entry, then `remember` the correct one)
- Stale project knowledge after major refactors

## Always Confirm

Before deleting, always confirm with the user what will be removed. Show the exact category and key.

For bulk deletion (removing all entries with a prefix), use `discover` first to list what will be affected, then delete one by one after confirmation.

## Usage

```
MCP tool: forget
  category: "people"
  key: "toby#old-email"
```

## Workflow: Correct a Memory

1. Recall the current value to confirm it exists
2. Forget the incorrect entry
3. Remember the corrected entry

```
recall → category: "people", prefix: "toby#email"
forget → category: "people", key: "toby#email"
remember → category: "people", key: "toby#email", content: "new-email@example.com"
```

## Restrictions

- Cannot delete from the `_schema` category directly
- Deleting a non-existent key silently succeeds (idempotent)
