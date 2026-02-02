---
name: teach
description: Capture knowledge from natural language. When the user says "remember that..." or tells you something worth retaining, parse it into structured memory automatically.
---

# Teach — Conversational Memory Capture

Parse natural language into structured, persistent memories. The user shouldn't need to know about categories or key formats — you infer the right structure from context.

## Auto-Trigger Patterns

Invoke this behavior **proactively** when you detect:

- "remember that...", "note that...", "keep in mind..."
- "from now on...", "always...", "never..."
- "my email is...", "the API key is...", "the endpoint is..."
- User states a fact, preference, or convention they expect you to retain
- User corrects your understanding of something ("actually, it's X not Y")

You do NOT need to wait for `/ferridyn-memory:teach` — act on these patterns immediately.

## Workflow

### Step 1: Identify What to Store

Extract from the user's statement:
- **What** — the fact, preference, decision, or convention
- **Category** — which domain it belongs to (see conventions below)
- **Key** — a hierarchical `segment#segment` key
- **Content** — concise, self-contained text

### Step 2: Check Existing Categories

```bash
fmemory discover
```

See what categories already exist. Prefer reusing existing categories over creating new ones.

### Step 3: Store the Memory

```bash
fmemory remember --category "{inferred category}" --key "{inferred key}" \
  --content "{extracted content}" --metadata "source: user taught, {date}"
```

If the category is new, fmemory will auto-infer a schema from your first write.

### Step 4: Confirm

Tell the user what you stored and how you categorized it. Keep it brief:

> Stored in **people**: `toby#email` = "toby@example.com"

## Category Inference Guide

| User Says... | Category | Key | Content |
|--------------|----------|-----|---------|
| "Toby's email is toby@example.com" | `people` | `toby#email` | toby@example.com |
| "We use tabs not spaces" | `preferences` | `code#indentation` | Tabs, not spaces |
| "The staging URL is staging.example.com" | `tools` | `staging#url` | staging.example.com |
| "Always run tests before committing" | `preferences` | `workflow#pre-commit` | Always run tests before committing |
| "We chose Postgres over SQLite for concurrency" | `decisions` | `database#postgres-over-sqlite` | Chose Postgres over SQLite for better concurrency support |
| "The auth token goes in X-Api-Key header" | `project` | `conventions#auth-header` | Auth token uses X-Api-Key header |
| "From now on, use async/await not callbacks" | `preferences` | `code#async-style` | Use async/await, not callbacks |

## When NOT to Store

- Trivial exchanges ("ok", "thanks", "sure")
- Information already visible in CLAUDE.md or AGENTS.md
- Temporary debugging state (use notepad instead)
- Large code blocks (store a summary + file path)
- Speculative or unconfirmed information — only store facts

## Handling Corrections

If the user says "actually, it's X not Y" and there's an existing memory with the wrong value:

1. Use `fmemory recall` to find the old entry
2. Use `fmemory forget` to remove it
3. Use `fmemory remember` to store the corrected value
4. Confirm: "Updated **people**: `toby#email` from old@example.com to new@example.com"

This is equivalent to the `/ferridyn-memory:update` skill but triggered conversationally.
