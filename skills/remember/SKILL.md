---
name: remember
description: Store a memory using the FerridynDB memory system. Guides the agent on what to store, how to choose categories and keys, and when NOT to store.
---

# Remember — Store a Memory

Use the `fmemory remember` command to persist knowledge across sessions.

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

## Category and Key Conventions

Categories are partition keys — broad semantic groupings:
- `project` — project structure, conventions, architecture
- `people` — contacts, roles, preferences
- `decisions` — architecture and design decisions
- `bugs` — bug patterns, fixes, workarounds
- `preferences` — user workflow preferences
- `tools` — tool configs, environment details

Keys use `#` hierarchy matching the category's schema:
- `people`: `{name}#{attribute}` → `toby#email`, `alice#role`
- `project`: `{area}#{topic}` → `structure#crate-layout`, `conventions#naming`
- `decisions`: `{area}#{decision}` → `auth#jwt-over-sessions`, `db#sqlite-over-postgres`

## Schema Inference

On first write to a **new category**, fmemory automatically infers a schema from the data. The schema defines the expected key format for future writes.

If you need a specific key format, use `fmemory define` first to explicitly set the schema.

## Validation

After a schema is set, fmemory validates keys on every write. If a key doesn't match, you'll get an error explaining the expected format with examples.

## Usage

```bash
fmemory remember --category "people" --key "toby#email" --content "toby@example.com" --metadata "source: conversation 2025-01-15"
```

The `--metadata` flag is optional.

## Tips

- Store the **why** not just the **what** — "chose JWT because sessions don't work with serverless" > "using JWT"
- Use descriptive keys — `auth#refresh-token-flow` not `auth#thing1`
- Include timestamps in metadata for time-sensitive information
- Prefer atomic memories (one fact per entry) over large blobs
