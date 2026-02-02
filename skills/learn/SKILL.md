---
name: learn
description: Deep codebase exploration that builds persistent project memory. Like /init but stores findings in FerridynDB for cross-session recall.
---

# Learn — Build Project Memory

Explore the codebase deeply and store structured findings as persistent memories. This is the FerridynDB equivalent of `/init` — but instead of writing a one-time CLAUDE.md, it builds incrementally updatable project knowledge.

## Schema

Category: `project`
Key format: `{area}#{topic}`

Areas:
- `structure` — directory layout, crate/module organization, entry points
- `conventions` — naming patterns, code style, error handling
- `architecture` — design patterns, data flow, key abstractions
- `dependencies` — external crates/packages and their purposes
- `build` — build system, CI/CD, test infrastructure
- `patterns` — recurring code patterns and idioms

## Process

### Step 0: Check Existing Memories

Before exploring, check if project memories already exist:

```bash
fmemory recall --category "project" --limit 5
```

If memories exist, ask the user:

**Question:** "Found existing project memories. What would you like to do?"

**Options:**
1. **Update** — Re-explore and update outdated entries
2. **Start fresh** — Clear project memories and rebuild from scratch
3. **Skip** — Keep existing memories as-is

If "Start fresh": recall all project memories and forget each one.

### Step 1: Define Schema

Ensure the project schema is defined:

```bash
fmemory define --category "project" \
  --description "Project structure, conventions, architecture, and build knowledge" \
  --sort_key_format "{area}#{topic}" \
  --segments '{"area": "knowledge area (structure, conventions, architecture, dependencies, build, patterns)", "topic": "specific topic within the area"}' \
  --examples "structure#crate-layout, conventions#naming, architecture#data-flow, dependencies#serde, build#test-commands"
```

### Step 2: Explore and Store

For each area, explore the codebase and store findings:

#### structure
- Scan directory layout with `ls` / `find`
- Identify languages, frameworks, entry points
- Store: `structure#directory-layout`, `structure#entry-points`, `structure#module-organization`

#### conventions
- Read a sample of source files
- Identify naming patterns (camelCase, snake_case, etc.)
- Error handling style (Result types, error crates, panic policy)
- Test conventions (location, naming, fixtures)
- Store: `conventions#naming`, `conventions#error-handling`, `conventions#testing`

#### architecture
- Read entry points and public APIs
- Identify key abstractions and their relationships
- Trace data flow through the system
- Store: `architecture#key-abstractions`, `architecture#data-flow`, `architecture#design-decisions`

#### dependencies
- Read Cargo.toml / package.json / requirements.txt
- Catalog major dependencies and their purposes
- Store: `dependencies#{dep-name}` for each major dependency

#### build
- Identify build commands, test commands, CI config
- Store: `build#commands`, `build#ci`, `build#test-infrastructure`

#### patterns
- Identify recurring code patterns
- Common idioms specific to the project
- Store: `patterns#error-propagation`, `patterns#testing-helpers`, etc.

### Step 3: Present Summary

After storing findings, present a summary to the user:

```
Project Memory Built!

Stored N memories across these areas:
  structure: M entries (directory layout, entry points, ...)
  conventions: M entries (naming, error handling, ...)
  architecture: M entries (key abstractions, data flow, ...)
  dependencies: M entries (serde, tokio, ...)
  build: M entries (commands, CI, ...)
  patterns: M entries (error propagation, ...)

These memories will be automatically recalled in future sessions
when questions relate to project structure or conventions.
```

## Agent Delegation

This skill is exploration-heavy. Delegate to specialized agents:

- **explore** agents for directory scanning and file discovery
- **architect** agent for identifying patterns and abstractions
- **researcher** agent for understanding dependency purposes

## Incremental Updates

The learn skill can be re-run to update specific areas. When updating:

1. Recall existing entries for that area
2. Compare with current codebase state
3. Forget outdated entries
4. Remember updated entries
