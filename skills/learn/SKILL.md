---
name: learn
description: Deep codebase exploration that builds persistent project memory. Like /init but stores findings in FerridynDB for cross-session recall.
---

# Learn — Build Project Memory

Explore the codebase deeply and store structured findings as persistent memories. This is the FerridynDB equivalent of `/init` — but instead of writing a one-time CLAUDE.md, it builds incrementally updatable project knowledge.

## Category

Category: `project`

Areas (used as keys or key prefixes):
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
fmemory recall --category project --limit 5
```

If memories exist, ask the user:

**Question:** "Found existing project memories. What would you like to do?"

**Options:**
1. **Update** — Re-explore and update outdated entries
2. **Start fresh** — Clear project memories and rebuild from scratch
3. **Skip** — Keep existing memories as-is

If "Start fresh": recall all project memories and forget each one.

### Step 1: Define Schema (if new)

If the `project` category doesn't exist yet, define its schema:

```bash
fmemory define --category project --description "Project structure, conventions, architecture, and build knowledge" \
  --attributes '[{"name":"area","type":"STRING","required":true},{"name":"topic","type":"STRING","required":true},{"name":"details","type":"STRING","required":true}]' \
  --auto-index
```

### Step 2: Explore and Store

For each area, explore the codebase and store findings using NL-first syntax:

#### structure
- Scan directory layout
- Identify languages, frameworks, entry points
- Store: `fmemory remember --category project "Project structure: src/ contains Rust source, scripts/ has TypeScript hooks..."`

#### conventions
- Read a sample of source files
- Identify naming patterns, error handling style, test conventions
- Store: `fmemory remember --category project "Naming convention: snake_case for Rust modules, camelCase for TypeScript..."`

#### architecture
- Read entry points and public APIs
- Identify key abstractions and their relationships
- Store: `fmemory remember --category project "Architecture: MCP server delegates to SchemaManager for inference..."`

#### dependencies
- Read Cargo.toml / package.json / requirements.txt
- Catalog major dependencies and their purposes
- Store: `fmemory remember --category project "Key dependency: rmcp 0.14 for MCP server framework..."`

#### build
- Identify build commands, test commands, CI config
- Store: `fmemory remember --category project "Build: cargo build --release for binaries, npm run build:scripts for hooks..."`

#### patterns
- Identify recurring code patterns
- Store: `fmemory remember --category project "Pattern: all LLM calls go through LlmClient trait for testability..."`

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
4. Remember updated entries using NL-first syntax
