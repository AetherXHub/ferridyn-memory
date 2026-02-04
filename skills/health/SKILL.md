---
name: health
description: Diagnose memory integrity — check schema coverage, index coverage, find empty categories, detect oversized stores, and suggest fixes.
---

# Health — Memory Diagnostics

Run a health check on the memory system. Identifies missing schemas, missing indexes, empty categories, oversized stores, and other issues that could degrade memory quality.

## Auto-Trigger Patterns

Consider running this when:

- Before a `/ferridyn-memory:learn` session (ensure clean slate)
- After bulk operations (verify nothing broke)
- User reports memory retrieval issues
- Periodically during long-running projects

## Workflow

### Step 1: Run Diagnostic Script

```bash
node ${CLAUDE_PLUGIN_ROOT}/scripts/dist/memory-health.mjs
```

This outputs a JSON health report with:
- `total_categories` — number of memory categories
- `total_entries` — total memories stored
- `schema_coverage` — percentage of categories with native partition schemas
- `index_coverage` — percentage of categories with secondary indexes
- `issues` — array of `{severity, category?, issue}` findings
- `categories` — per-category breakdown: name, entry count, schema status, attributes, indexes

### Step 2: Interpret Results

Present findings by severity:

**Errors** (must fix):
- CLI unavailable — memory system is not running (server required)

**Warnings** (should fix):
- Missing schema — category has no native partition schema, data is unstructured. Fix: use `fmemory define` to set attributes.
- Missing indexes — category has no secondary indexes, NL queries will be slower. Fix: use `fmemory define --auto-index` to add indexes.
- Failed to discover/count — possible server connection issue

**Info** (awareness):
- Empty category — exists but has no entries (may have been cleared)
- Large category — over 100 entries, consider pruning old entries

### Step 3: Suggest Fixes

For each issue, suggest the appropriate action:

| Issue | Fix |
|-------|-----|
| No schema defined | Store data with `fmemory remember` (auto-infers) or `fmemory define --attributes [...]` |
| No indexes | `fmemory define --category <cat> --auto-index` |
| Empty category | Either populate it or ignore (harmless) |
| Large category | Review entries with `/ferridyn-memory:browse`, prune stale ones with `forget` |
| CLI unavailable | Run `/ferridyn-memory:setup` to rebuild and restart server |

### Step 4: Present Summary

```
Memory Health Check

  Status: 2 warnings, 1 info
  Categories: 5 (4 with schemas, 3 with indexes)
  Total entries: 47

  Issues:
    [WARNING] temp — No schema defined
    [WARNING] temp — No secondary indexes
    [INFO] project — Large category: 45 entries

  Recommendations:
    - Define a schema for 'temp' or remove it if unused
    - Review 'project' entries — consider archiving old ones
```

## When to Run

- User asks "is my memory healthy" or "check memory"
- Before major operations (learn, bulk update)
- When recall results seem wrong or incomplete
- After upgrading the plugin
