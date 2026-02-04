---
name: browse
description: Interactively explore the memory structure — list categories, inspect schemas and indexes, drill into entries.
---

# Browse — Explore Memory Structure

Use the `fmemory discover` command to navigate the memory structure.

## Exploration Workflow

### Step 1: List All Categories

```bash
fmemory discover
```

Returns all categories with their schema descriptions, attribute counts, and index counts:
```
contacts: People and contacts (3 attributes, 2 indexes)
project: Project structure and conventions (4 attributes, 1 index)
decisions: Architectural decisions (4 attributes, 1 index)
```

### Step 2: Drill Into a Category

```bash
fmemory discover --category contacts
```

Returns keys, attributes, and indexes within that category:
```
contacts: People and contacts

Attributes:
  name (STRING, required)
  email (STRING, required)
  role (STRING)

Indexes:
  name_idx on name
  email_idx on email

Keys: alice, bob, carol, toby
```

### Step 3: Read Specific Entries

```bash
fmemory recall --category contacts --key toby
```

Returns the structured item with all attributes:
```
toby (contacts)
  Name: Toby
  Email: toby@example.com
  Role: backend engineer
```

## Presentation

When presenting browse results to the user:

- Format categories as a readable list with descriptions, attribute counts, and index counts
- Show schema attributes and indexes when drilling into a category
- Offer to drill deeper ("Want to see entries for a specific category?")
- For large result sets, summarize and offer filtering

## When to Browse

- User asks "what do we have stored?" or "show me the memories"
- Before storing new data, to understand existing structure
- When debugging memory retrieval issues
- To verify data after bulk operations
