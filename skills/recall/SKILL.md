---
name: recall
description: Retrieve memories from the DynaMite memory system. Supports precise category+prefix lookups and natural language queries.
---

# Recall — Retrieve Memories

Use the MCP `recall` tool to retrieve stored knowledge.

## Two Modes

### 1. Precise Mode (category + prefix)

When you know exactly what to look for:

```
MCP tool: recall
  category: "people"
  prefix: "toby"
```

This returns all entries in `people` where the key starts with `toby` (e.g. `toby#email`, `toby#role`, `toby#phone`).

Without a prefix, returns all entries in the category:

```
MCP tool: recall
  category: "project"
```

### 2. Natural Language Mode (query)

When the user's request implies memory but doesn't specify a category:

```
MCP tool: recall
  query: "Toby's email address"
```

The server sends all known schemas to Claude Haiku, which resolves the query to the right category and prefix. This is the preferred mode for agent-driven memory retrieval.

## When to Use Each Mode

| Situation | Mode |
|-----------|------|
| User asks "what's Toby's email?" | `query: "Toby's email"` |
| Hook needs context for a prompt | `query: "{summarized prompt}"` |
| Agent knows the exact category | `category: "project", prefix: "conventions"` |
| Listing all entries in a category | `category: "bugs"` |
| Fallback when query fails | `category` + `prefix` (discover first) |

## Fallback Strategy

If natural language recall returns no results:

1. Try `discover` (no category) to list all categories
2. Pick the most relevant category
3. Try `discover` with that category to see key prefixes
4. Use precise recall with the right prefix

## Usage in Hooks

The `memory-retrieval.mjs` hook automatically calls recall with a natural language query derived from the user's prompt. This injects relevant context before the conversation starts.

## Limit

Default limit is 20 results. For bulk retrieval, set a higher limit:

```
MCP tool: recall
  category: "project"
  limit: 100
```
