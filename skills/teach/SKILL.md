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
- **Content** — descriptive natural language text

### Step 2: Check Existing Categories

```bash
fmemory discover
```

See what categories already exist. Prefer reusing existing categories over creating new ones.

### Step 3: Store the Memory

Use NL-first syntax — let fmemory extract structure from your description:

```bash
fmemory remember --category contacts "Toby is a backend engineer, email toby@example.com"
```

If the category is new, fmemory will auto-infer a schema with typed attributes and indexes from your first write.

### Step 4: Confirm

Tell the user what you stored and how you categorized it. Keep it brief:

> Stored in **contacts**: Toby — backend engineer, toby@example.com

## Category Inference Guide

| User Says... | Category | Store Command |
|--------------|----------|---------------|
| "Toby's email is toby@example.com" | `contacts` | `fmemory remember --category contacts "Toby's email is toby@example.com"` |
| "We use tabs not spaces" | `preferences` | `fmemory remember --category preferences "Code indentation: tabs, not spaces"` |
| "The staging URL is staging.example.com" | `tools` | `fmemory remember --category tools "Staging URL is staging.example.com"` |
| "Always run tests before committing" | `preferences` | `fmemory remember --category preferences "Always run tests before committing"` |
| "We chose Postgres over SQLite for concurrency" | `decisions` | `fmemory remember --category decisions "Chose Postgres over SQLite for better concurrency support"` |
| "The auth token goes in X-Api-Key header" | `project` | `fmemory remember --category project "Auth token uses X-Api-Key header"` |
| "From now on, use async/await not callbacks" | `preferences` | `fmemory remember --category preferences "Use async/await, not callbacks"` |

## When NOT to Store

- Trivial exchanges ("ok", "thanks", "sure")
- Information already visible in CLAUDE.md or AGENTS.md
- Temporary debugging state (use notepad instead)
- Large code blocks (store a summary + file path)
- Speculative or unconfirmed information — only store facts

## Handling Corrections

If the user says "actually, it's X not Y" and there's an existing memory with the wrong value:

1. Use `fmemory recall --query "..."` to find the old entry
2. Use `fmemory forget` to remove it
3. Use `fmemory remember` to store the corrected value with NL-first syntax
4. Confirm: "Updated **contacts**: Toby's email changed from old@example.com to new@example.com"

This is equivalent to the `/ferridyn-memory:update` skill but triggered conversationally.
