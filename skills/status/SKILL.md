---
name: status
description: Quick overview of what's in memory — categories, schemas, indexes, entry counts, and sample keys. Orient yourself at session start or check what's been stored.
---

# Status — Memory Overview

Get a fast summary of everything in memory. Useful for orientation at session start, verifying what's stored, or deciding what to explore further.

## Auto-Trigger Patterns

Invoke this behavior **proactively** when:

- Beginning of a session and you want to understand stored context
- User asks "what do you know about this project" or "what's in memory"
- Before a `/ferridyn-memory:learn` session (to avoid duplicating existing knowledge)
- After a bulk operation (to verify results)
- User asks "have we stored anything about X"

## Workflow

### Step 1: Run Stats Script (Preferred)

For structured data, run the stats utility:

```bash
node ${CLAUDE_PLUGIN_ROOT}/scripts/dist/memory-stats.mjs
```

This outputs JSON with `total_categories`, `total_entries`, and per-category details (name, description, attributes, indexes, entry_count, sample_keys).

### Step 1 (Alternative): Manual CLI Discovery

If the stats script is unavailable, use fmemory directly:

```bash
fmemory discover
```

Then for each category:

```bash
fmemory discover --category "{cat}"
```

### Step 2: Present Summary

Format as a readable overview showing schemas and indexes:

```
Memory Status: 4 categories, ~47 entries

  contacts (8 entries)
    Schema: People and contacts (3 attributes, 2 indexes)
    Keys: alice, bob, carol, toby

  project (22 entries)
    Schema: Project structure and conventions (4 attributes, 1 index)
    Keys: architecture, build, conventions, dependencies, patterns, structure

  decisions (12 entries)
    Schema: Architecture and design decisions (4 attributes, 1 index)
    Keys: api, auth, database, frontend, testing

  preferences (5 entries)
    Schema: User workflow preferences (2 attributes, 0 indexes)
    Keys: code, editor, workflow
```

### Step 3: Offer Next Steps

Based on what's in memory, suggest:

- **If empty**: "No memories stored yet. Use `/ferridyn-memory:learn` to build project memory, or just start working — I'll remember important things automatically."
- **If sparse**: "Memory is light. Want me to explore the codebase and build knowledge with `/ferridyn-memory:learn`?"
- **If populated**: "Want to drill into a specific category? Use `/ferridyn-memory:browse` or ask about anything stored."

## Quick Check Variant

Sometimes you just need to know if a specific topic is stored, not a full overview:

```bash
fmemory -p "database configuration"
```

If results come back -> topic is covered.
If nothing -> consider asking the user and storing the answer (context skill behavior).

## When NOT to Run Full Status

- Don't run a full status on every prompt (the hook already handles ambient retrieval)
- Don't run status if you're about to do a targeted recall anyway
- Save full status for orientation moments (session start, topic switches, user requests)
