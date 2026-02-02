---
name: context
description: Before starting complex work, pull all relevant memories. If expected knowledge is missing, ask the user and store their answer for future sessions.
---

# Context — Pre-Task Memory Retrieval

Before diving into complex work, check memory for relevant context. This is a deeper, targeted pull compared to the automatic hook — you know specifically what you need.

## Auto-Trigger Patterns

Invoke this behavior **proactively** when:

- You're about to start a complex task (new feature, refactor, debugging)
- You're switching to a different area of the codebase
- You need to know project conventions before writing code
- You're about to make a decision and want to check for prior decisions in the same area
- The user asks "what do we know about X"

You do NOT need to wait for `/ferridyn-memory:context` — pull context before starting work.

## Workflow

### Step 1: Identify What You Need

Before starting work, ask yourself:
- What area of the codebase is this in?
- Are there conventions I should follow?
- Were there prior decisions about this area?
- Are there known bugs or gotchas?
- Does the user have preferences for this kind of work?

### Step 2: Query Memory

Use natural language recall for broad context:

```
MCP tool: recall
  query: "{description of what you need to know}"
```

Or use precise recall if you know the category:

```
MCP tool: recall
  category: "project"
  prefix: "conventions"
```

For multiple areas, make multiple calls:

```
MCP tool: recall
  category: "decisions"
  prefix: "auth"

MCP tool: recall
  category: "preferences"

MCP tool: recall
  category: "bugs"
  prefix: "auth"
```

### Step 3: Browse If Needed

If recall doesn't find enough context, browse the full structure:

```
MCP tool: discover
  (no category)
```

Then drill into relevant categories:

```
MCP tool: discover
  category: "project"
```

### Step 4: Handle Missing Knowledge (Critical)

**If you expect information to exist but it's missing, ASK the user.**

Examples:
- About to write database queries but no memory of the ORM or query style → Ask: "What ORM/query approach does this project use?"
- About to add error handling but no convention stored → Ask: "What's your preferred error handling pattern?"
- Need to know the deployment target but nothing stored → Ask: "Where does this project deploy?"

After the user answers, **store it immediately**:

```
MCP tool: remember
  category: "project"
  key: "conventions#error-handling"
  content: "{user's answer}"
  metadata: "source: user clarification, {date}"
```

This way, no future session needs to ask the same question.

### Step 5: Apply Context

Use the retrieved memories to inform your work. Mention relevant context when it influences your approach:

> Based on stored conventions, this project uses custom error enums per module. I'll follow that pattern for the new auth module.

## When to Pull Context

| Situation | What to Query |
|-----------|--------------|
| Starting a new feature | `project/conventions`, `decisions/{area}`, `preferences` |
| Debugging | `bugs/{area}`, `project/architecture` |
| Refactoring | `project/architecture`, `decisions`, `project/conventions` |
| Writing tests | `project/conventions#testing`, `preferences` |
| Configuring tools | `tools`, `project/build` |
| Working with people | `people/{name}` |

## Depth Calibration

Don't over-retrieve. Match depth to task:

| Task Complexity | Retrieval Depth |
|----------------|----------------|
| Quick fix | 1 recall (relevant area only) |
| Standard feature | 2-3 recalls (conventions + area decisions + preferences) |
| Complex work | Discover + 3-5 recalls (full context sweep) |
