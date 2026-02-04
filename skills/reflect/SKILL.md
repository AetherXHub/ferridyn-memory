---
name: reflect
description: After completing significant work, extract and persist learnings — decisions made, patterns discovered, gotchas encountered, and preferences revealed.
---

# Reflect — Post-Task Learning Extraction

After completing significant work, pause and extract what was learned. Store it as persistent memory so future sessions benefit without re-discovery.

## Auto-Trigger Patterns

Invoke this behavior **proactively** when:

- You just completed a multi-step task (feature implementation, refactor, migration)
- You debugged and fixed a hard bug
- You made or participated in an architectural decision
- You completed a refactoring session
- A long conversation is wrapping up with substantial work done
- You discovered a non-obvious project behavior or convention

You do NOT need to wait for `/ferridyn-memory:reflect` — trigger this after significant work.

## What to Extract

Ask yourself: **"What would I want to know if I started a fresh session on this codebase?"**

### High-Value Learnings

| Type | Example | Category |
|------|---------|----------|
| Decisions | "Chose JWT over sessions because of serverless deployment" | `decisions` |
| Bug patterns | "Race condition in auth middleware when tokens expire mid-request" | `bugs` |
| Gotchas | "The test suite requires REDIS_URL even for unit tests" | `project` |
| Conventions | "All API routes use kebab-case, handlers use camelCase" | `project` |
| Preferences | "User prefers explicit error types over anyhow" | `preferences` |
| Architecture | "Events flow: API -> queue -> processor -> DB -> webhook" | `project` |
| Workarounds | "Must pin openssl-sys to 0.9.80 on ARM builds" | `bugs` |

### Low-Value (Skip These)

- "I read the file and it contains..."
- "The build passed" (transient state)
- Information already in CLAUDE.md
- Step-by-step task progress (use todos for that)

## Workflow

### Step 1: Review What Was Accomplished

Look back at the conversation. Identify:
- What was the task?
- What decisions were made along the way?
- What was surprising or non-obvious?
- What gotchas or bugs were encountered?
- Did the user express any preferences?

### Step 2: Check Existing Memories

```bash
fmemory discover
```

See what categories exist. Avoid storing duplicates of things already known.

### Step 3: Store Each Learning

For each learning worth persisting, use NL-first syntax:

```bash
fmemory remember --category decisions "Chose 15-minute refresh tokens for security/UX balance. Alternatives: session cookies (rejected, serverless), long-lived tokens (rejected, security)"
```

```bash
fmemory remember --category bugs "Integration tests fail intermittently when DB pool exhausts under concurrent test runners"
```

```bash
fmemory remember --category project "Convention: custom error enums per module, not anyhow"
```

### Step 4: Summarize

Tell the user what was stored:

```
Reflected on this session and stored 3 memories:
  - decisions: token-rotation — Chose 15-minute refresh tokens for security/UX balance
  - bugs: flaky-test-db-pool — Integration tests fail intermittently when DB pool exhausts
  - project: error-types — Custom error enums per module, not anyhow
```

## Depth Calibration

Match reflection depth to work complexity:

| Work Done | Reflection Depth |
|-----------|-----------------|
| Small bug fix | 0-1 memories (store the bug pattern if non-trivial) |
| Feature implementation | 2-4 memories (decisions, conventions, gotchas) |
| Major refactor | 3-6 memories (architecture changes, migration patterns, breaking changes) |
| Debugging session | 1-3 memories (root cause, diagnosis approach, fix pattern) |

## Integration with Other Skills

- If reflection surfaces a decision -> also consider `/ferridyn-memory:decide` for richer logging
- If reflection finds outdated memories -> use `/ferridyn-memory:update` to correct them
- If you're unsure about something discovered -> ask the user before storing
