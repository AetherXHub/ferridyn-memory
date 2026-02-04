---
name: remember
description: Store a memory using the FerridynDB memory system. Guides the agent on what to store, how to choose categories, and when NOT to store.
---

# Remember — Store a Memory

Use the `fmemory remember` command to persist knowledge across sessions. fmemory is NL-first — describe what you want to store in natural language and let the system extract structure.

## When to Store

- Architecture decisions and rationale
- User preferences and workflows
- Bug patterns and their fixes
- Project-specific knowledge (conventions, configs, gotchas)
- Contact information and relationships
- Tool configurations and environment details
- Recurring task patterns

## When NOT to Store

- Trivial exchanges ("hello", "thanks")
- Temporary debugging state (use notepad instead)
- Information already in CLAUDE.md or AGENTS.md
- Large code blocks (store a summary + file path instead)
- Speculative or unconfirmed information

## NL-First Syntax

The primary interface is natural language. Just describe what you want to remember:

```bash
fmemory remember "Toby is a backend engineer, email toby@example.com"
```

Optionally specify a category:

```bash
fmemory remember --category contacts "Toby is a backend engineer, email toby@example.com"
```

Or provide both category and key for full control:

```bash
fmemory remember --category contacts --key toby "backend engineer at Example Corp"
```

## Categories

Categories are partition keys — broad semantic groupings:
- `project` — project structure, conventions, architecture
- `contacts` — people, roles, contact info
- `decisions` — architecture and design decisions
- `bugs` — bug patterns, fixes, workarounds
- `preferences` — user workflow preferences
- `tools` — tool configs, environment details

## Structured Data

Items have typed attributes (name, email, role, etc.), not flat content strings. Keys are simple identifiers (`toby`), not hierarchical formats.

**Before (old):** `{ "category": "people", "key": "toby#email", "content": "toby@example.com" }`

**Now:** `{ "category": "contacts", "key": "toby", "name": "Toby", "email": "toby@example.com", "role": "backend engineer" }`

## Automatic Schema Inference

On first write to a **new category**, fmemory uses Claude Haiku to:
1. Infer typed attributes from the data (e.g., `name: STRING`, `email: STRING`)
2. Suggest secondary indexes for fast attribute-based lookups
3. Create a native partition schema for the category

Subsequent writes to the same category are parsed against the existing schema.

If you need a specific schema, use `fmemory define` first to explicitly set attributes and indexes.

## Tips

- Store descriptive NL text — let the system extract structure
- Store the **why** not just the **what** — "chose JWT because sessions don't work with serverless" > "using JWT"
- Prefer atomic memories (one fact per entry) over large blobs
- No need to worry about key formats — the system generates appropriate keys from your NL input
