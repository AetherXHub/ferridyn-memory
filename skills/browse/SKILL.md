---
name: browse
description: Interactively explore the memory structure — list categories, inspect schemas, drill into entries.
---

# Browse — Explore Memory Structure

Use the MCP `discover` tool to navigate the memory hierarchy.

## Exploration Workflow

### Step 1: List All Categories

```
MCP tool: discover
  (no category)
```

Returns all categories with their schema descriptions (if defined):
```
- people: People and contacts (key: {name}#{attribute})
- project: Project structure and conventions (key: {area}#{topic})
- decisions: Architecture decisions (key: {area}#{decision})
```

### Step 2: Drill Into a Category

```
MCP tool: discover
  category: "people"
```

Returns sort key prefixes within that category:
```
["toby", "alice", "bob"]

Schema: People and contacts
Key format: {name}#{attribute}
Examples: ["toby#email", "alice#role"]
```

### Step 3: Read Specific Entries

```
MCP tool: recall
  category: "people"
  prefix: "toby"
```

Returns all entries for Toby:
```json
[
  {"category": "people", "key": "toby#email", "content": "toby@example.com"},
  {"category": "people", "key": "toby#role", "content": "Backend engineer"}
]
```

## Presentation

When presenting browse results to the user:

- Format categories as a readable list with descriptions
- Show key format and examples from schemas
- Offer to drill deeper ("Want to see entries for a specific category?")
- For large result sets, summarize and offer filtering

## When to Browse

- User asks "what do we have stored?" or "show me the memories"
- Before storing new data, to understand existing structure
- When debugging memory retrieval issues
- To verify data after bulk operations
