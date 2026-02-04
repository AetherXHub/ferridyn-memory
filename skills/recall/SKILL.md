---
name: recall
description: Retrieve memories from the FerridynDB memory system. Supports exact lookup, category scan, and natural language queries with index-optimized retrieval.
---

# Recall — Retrieve Memories

Use the `fmemory recall` command to retrieve stored knowledge.

## Three Modes

### 1. Exact Lookup (category + key)

When you know exactly what to retrieve:

```bash
fmemory recall --category contacts --key toby
```

Returns the full structured item for Toby with all attributes:

```
toby (contacts)
  Name: Toby
  Email: toby@example.com
  Role: backend engineer
```

### 2. Category Scan (category only)

List all entries in a category:

```bash
fmemory recall --category contacts
```

### 3. Natural Language Query

When the user's request implies memory but doesn't specify a category:

```bash
fmemory recall --query "Toby's email address"
```

Or use the `-p` flag for natural language:

```bash
fmemory -p "Toby's email address"
```

fmemory resolves the query using known schemas and secondary indexes. When the query targets an indexed attribute, fmemory uses the secondary index for fast lookups instead of scanning.

## When to Use Each Mode

| Situation | Mode |
|-----------|------|
| User asks "what's Toby's email?" | `--query "Toby's email"` |
| Hook needs context for a prompt | `--query "{summarized prompt}"` |
| Agent knows the exact item | `--category contacts --key toby` |
| Listing all entries in a category | `--category contacts` |
| Fallback when query returns nothing | `discover` then exact lookup |

## Fallback Strategy

If natural language recall returns no results:

1. Run `fmemory discover` to list all categories with their schemas and index counts
2. Pick the most relevant category
3. Run `fmemory discover --category <cat>` to see keys, attributes, and indexes
4. Use exact lookup with `--category` and `--key`

## Prose Output

Results are human-readable by default:

```
toby (contacts)
  Name: Toby
  Email: toby@example.com
  Role: backend engineer
```

Use `--json` for machine-readable output when needed by scripts.

## Usage in Hooks

The `memory-retrieval.mjs` hook automatically calls recall with a natural language query derived from the user's prompt. This injects relevant context before the conversation starts.

## Limit

Default limit is 20 results. For bulk retrieval, set a higher limit:

```bash
fmemory recall --category project --limit 100
```
