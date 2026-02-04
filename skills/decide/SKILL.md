---
name: decide
description: Log significant decisions with rationale, alternatives considered, and constraints. Decisions are the highest-value memories — they prevent re-debating in future sessions.
---

# Decide — Decision Logging

When a significant decision is made, log it with full context. Decisions are the highest-value memories — they answer "why did we do it this way?" without forcing future sessions to re-debate.

## Auto-Trigger Patterns

Invoke this behavior **proactively** when:

- An architectural decision is made (system design, data flow, component boundaries)
- A technology choice is resolved (library A vs B, framework selection)
- A design pattern is chosen (error handling strategy, state management approach)
- A trade-off is resolved (performance vs simplicity, consistency vs flexibility)
- User says "let's go with X" after discussing alternatives
- A convention is established ("from now on, we'll do X")

You do NOT need to wait for `/ferridyn-memory:decide` — log decisions as they happen.

## What Makes a Decision Worth Logging

**Log it if:**
- It was debated or had alternatives
- It affects future work in the same area
- Someone might ask "why" later
- Reversing it would be non-trivial

**Skip it if:**
- It's trivial (variable naming, minor formatting)
- It's forced (only one option exists)
- It's temporary ("let's try X for now")

## Workflow

### Step 1: Check for Prior Decisions

Before logging, check if there's an existing decision in the same area:

```bash
fmemory recall --query "decisions about {area}"
```

If a prior decision exists in the same area, note whether the new decision supersedes, refines, or conflicts with it.

### Step 2: Structure the Decision

A well-structured decision memory includes:

- **Decision**: What was decided
- **Rationale**: Why this option was chosen
- **Alternatives**: What else was considered and why it was rejected
- **Constraints**: What factors shaped the decision
- **Scope**: What this affects

### Step 3: Store

Store as a descriptive NL document. The system extracts structured attributes (decision, rationale, alternatives, constraints) automatically:

```bash
fmemory remember --category decisions "JWT refresh strategy: Chose 15-minute access tokens with 7-day refresh tokens. Rationale: balance security and UX for serverless deployment. Alternatives considered: session cookies (rejected, incompatible with serverless), long-lived tokens (rejected, security risk). Constraints: must work across multiple API gateways."
```

### Step 4: Confirm

> Logged decision: **decisions** — JWT refresh strategy
> Chose 15-minute access tokens with 7-day refresh tokens. Alternatives: session cookies (rejected, serverless), long-lived tokens (rejected, security).

## Decision Content Template

For consistency, structure decision content as a descriptive paragraph including:

```
{area}: {decision summary}. Rationale: {why}. Alternatives considered: {what else, why rejected}. Constraints: {what shaped this}.
```

Example:
```
Database engine: Use Postgres with Diesel ORM. Rationale: need ACID transactions for payment processing, Diesel provides compile-time query validation. Alternatives: SQLite (insufficient concurrency), MongoDB (no ACID), raw SQL (maintenance burden). Constraints: must support concurrent writes from 3 services, team familiar with SQL.
```

## Superseding Decisions

When a new decision replaces an old one:

1. Recall the old decision
2. Forget it
3. Store the new decision, including "Supersedes: {old decision summary}" in the content
4. Mention why the original decision was revisited

## Integration with Reflect

The `/ferridyn-memory:reflect` skill may surface decisions made during a task. If reflect identifies a decision, it may delegate to this skill's workflow for richer logging.
