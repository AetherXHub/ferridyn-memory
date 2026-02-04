---
name: forget
description: Remove a specific memory from the FerridynDB memory system. Use when information is outdated, incorrect, or explicitly requested to be removed.
---

# Forget — Remove a Memory

Use the `fmemory forget` command to remove specific memories.

## When to Remove

- Outdated information (old email, changed role, deprecated pattern)
- User explicitly asks to remove something
- Corrected data (delete the wrong entry, then `remember` the correct one)
- Stale project knowledge after major refactors

## Always Confirm

Before deleting, always confirm with the user what will be removed. Show the exact category and key.

For bulk deletion (removing all entries in a category), use `fmemory discover` first to list what will be affected, then delete one by one after confirmation.

## Usage

```bash
fmemory forget --category contacts --key toby
```

## Workflow: Correct a Memory

1. Recall the current value to confirm it exists
2. Forget the incorrect entry
3. Remember the corrected entry

```bash
fmemory recall --category contacts --key toby
fmemory forget --category contacts --key toby
fmemory remember --category contacts --key toby "backend engineer at New Corp, email toby@newcorp.com"
```

## Restrictions

- Deleting a non-existent key silently succeeds (idempotent)
