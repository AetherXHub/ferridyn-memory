# FerridynDB: Structured Content & Secondary Indexes

**Product Requirements Document**

**Requesting Team:** ferridyn-memory (Claude Code plugin)
**Date:** 2026-02-01
**Priority:** High — this is the single biggest capability gap blocking the next evolution of agentic memory.

---

## Executive Summary

ferridyn-memory is a Claude Code plugin that gives AI agents persistent, schema-aware memory backed by FerridynDB. Today the plugin stores memories as partition key (category) + sort key (hierarchical name) + opaque content string. The agent can only retrieve memories if it knows the exact category and sort key prefix.

This works for simple recall but breaks down for **associative recall** — the way agents (and humans) actually think. An agent might need to find a contact by name, email, project, or role. A decision by the technology it concerns or the date it was made. A project by its tech stack or team members.

We need two capabilities from FerridynDB:

1. **Structured content with schema enforcement** — Content stored as typed JSON documents with a defined shape per table/partition, validated on write.
2. **Secondary indexes on content fields** — The ability to query items by fields within the content document, not just by partition key and sort key.

These two features together enable multi-path recall without application-layer workarounds, and position FerridynDB as a first-class document store for agentic applications.

---

## Background: Current FerridynDB Capabilities

### What Exists Today

| Capability | Status |
|---|---|
| Partition key (string) | Supported |
| Sort key (string, optional) | Supported |
| Sort key prefix scan (`begins_with`) | Supported |
| Content field | Opaque `serde_json::Value` — no schema, no validation |
| Secondary indexes | Not supported |
| Content field queries | Not supported |
| Batch operations | Not supported |
| Transactions | Not supported |

### Current API Surface (7 operations)

| Operation | Description |
|---|---|
| `create_table` | Define table with partition key + optional sort key |
| `put_item` | Upsert a document |
| `get_item` | Exact lookup by partition key + sort key |
| `query` | Partition key match + optional sort key prefix scan |
| `delete_item` | Remove by exact keys |
| `list_partition_keys` | Discover all distinct partition keys in a table |
| `list_sort_key_prefixes` | Discover sort key first-segments within a partition |

### The Gap

The only query paths are:
- Exact match on partition key + sort key → `get_item`
- Partition key + sort key prefix → `query` with `begins_with`

There is no way to query by any field in the content document. If an item is stored as:

```json
{
  "category": "contacts",
  "key": "toby",
  "content": {"email": "toby@example.com", "role": "backend engineer", "projects": ["ferridyndb"]}
}
```

You cannot ask: "find all contacts where email = toby@example.com" or "find all contacts where projects contains ferridyndb". The only retrieval path is knowing the category and key.

---

## Requirements

### Requirement 1: Structured Content Schema

#### Problem

Content is currently an opaque `serde_json::Value`. The database stores whatever JSON the caller provides with no validation. This means:
- No guarantee of consistent document shape within a partition
- No ability for the database to reason about content fields (needed for indexing)
- Schema enforcement lives in the application layer (our plugin's Haiku-powered inference), which is fragile and non-deterministic

#### Proposed Capability

Allow defining a **content schema** per table (or per partition key value) that describes the expected shape of documents.

#### API Sketch

```rust
// Option A: Schema per table (simpler)
db.create_table("memories")
    .partition_key("category", KeyType::String)
    .sort_key("key", KeyType::String)
    .content_schema(json!({
        "type": "object",
        "properties": {
            "content": {"type": "object"},  // The actual data, shape varies by partition
        },
        "required": ["content"]
    }))
    .execute()?;

// Option B: Schema per partition key value (more flexible, what we actually need)
db.define_partition_schema("memories", "contacts", json!({
    "type": "object",
    "properties": {
        "name": {"type": "string"},
        "role": {"type": "string"},
        "email": {"type": "string"},
        "projects": {"type": "array", "items": {"type": "string"}},
        "notes": {"type": "string"}
    },
    "required": ["name"]
}))?;
```

#### Behavior

| Aspect | Requirement |
|---|---|
| **Validation on write** | `put_item` validates content against the schema for that partition. Rejects malformed documents with a clear error. |
| **Schema evolution** | Adding new optional fields to a schema should not invalidate existing documents. Removing or changing field types requires explicit migration (or a force flag). |
| **Schema storage** | Schemas are stored in the database itself (metadata table or system namespace). Not in application code. |
| **Schema discovery** | API to retrieve the schema for a table or partition: `get_schema(table, partition_key?) -> Option<Schema>`. |
| **Unschema'd partitions** | Partitions without a defined schema continue to accept arbitrary JSON (backwards compatible). |
| **Schema format** | JSON Schema (draft 7 or later) is the obvious choice. Keeps things standard. |

#### Why Per-Partition Schemas

In our use case, a single table (`memories`) holds many categories (partitions): contacts, decisions, projects, patterns. Each has a different content shape. A table-level schema would either be too loose (allowing anything) or require separate tables per category (losing the unified query surface).

Per-partition schemas mean `contacts` can enforce `{name, email, role, projects}` while `decisions` enforces `{decision, rationale, alternatives, date}` — in the same table.

#### What We Don't Need

- Schema migration tooling (we can handle this in the plugin layer)
- Schema versioning (nice to have, not blocking)
- Nested object validation beyond one level deep (flat-ish documents are fine)

---

### Requirement 2: Secondary Indexes on Content Fields

#### Problem

An AI agent recalls information associatively. It might think of a person by name, email, role, or project. A decision by its technology area or outcome. Today, the only retrieval path is partition key + sort key prefix. Finding a contact by email requires scanning all contacts and filtering in application code — or maintaining hand-rolled cross-reference entries.

#### Proposed Capability

Define secondary indexes on fields within the content document. The database maintains these indexes automatically on writes and deletes, and supports querying against them.

#### API Sketch

**Index Definition:**

```rust
// Define an index on a content field
db.create_index("memories", "contacts_email")
    .partition("contacts")          // Scoped to this partition key value
    .field("email")                 // Index the "email" field in content
    .execute()?;

// Array field index (each element indexed separately)
db.create_index("memories", "contacts_projects")
    .partition("contacts")
    .field("projects")              // Index each element of the array
    .execute()?;

// Cross-partition index (index a field across ALL partitions)
db.create_index("memories", "global_tags")
    .field("tags")                  // Index "tags" field regardless of partition
    .execute()?;
```

**Querying via Index:**

```rust
// Find contacts by email
db.query("memories")
    .partition_key("contacts")
    .filter("email", "=", "toby@example.com")
    .execute()?;
// Returns: [{category: "contacts", key: "toby", content: {...}}]

// Find all entries mentioning a project (array contains)
db.query("memories")
    .partition_key("contacts")
    .filter("projects", "contains", "ferridyndb")
    .execute()?;
// Returns: all contacts involved with ferridyndb

// Cross-partition search
db.query_index("memories", "global_tags")
    .filter("tags", "contains", "ferridyndb")
    .execute()?;
// Returns: items from ANY partition tagged with ferridyndb
```

#### Index Types Needed

| Index Type | Description | Use Case |
|---|---|---|
| **Exact match on scalar field** | Index a string/number field for `=` queries | Find contact by email |
| **Contains on array field** | Index each element of an array field | Find contacts by project, by tag |
| **Cross-partition** | Index a field across all partition key values | Global search by tag or term |

#### Index Types NOT Needed (for now)

| Index Type | Why Not |
|---|---|
| Full-text search | NL resolution via LLM handles fuzzy queries |
| Range queries on content fields | Agentic memory is mostly exact match / contains |
| Composite indexes (multi-field) | Single-field indexes with application-layer filtering is sufficient |
| Unique indexes | No uniqueness constraints needed |

#### Behavior

| Aspect | Requirement |
|---|---|
| **Automatic maintenance** | Indexes update automatically on `put_item` and `delete_item`. No manual rebuild. |
| **Consistency** | Index entries are consistent with the base data. A `put_item` followed by an index query must reflect the write. (Doesn't need to be transactional — eventual consistency within a single-process context is fine, but immediate consistency is preferred for embedded use.) |
| **Index on write cost** | Acceptable to have slower writes in exchange for fast indexed reads. Our write volume is low (tens of writes per session). |
| **Null/missing fields** | Items where the indexed field is missing or null are simply not indexed (not an error). |
| **Schema requirement** | Indexes SHOULD require a content schema on the partition (so the database knows the field type). Creating an index on an unschema'd partition could either error or work best-effort. |
| **Index discovery** | API to list indexes for a table: `list_indexes(table) -> Vec<IndexInfo>`. |
| **Index deletion** | API to remove an index: `delete_index(table, index_name)`. |

#### Server Protocol

Both the direct API (`FerridynDB`) and the client/server protocol (`FerridynClient`) need to support these operations. The plugin uses both modes:
- Direct mode for testing and fallback
- Server mode for production (shared access via Unix socket)

The server protocol needs new message types for:
- `CreateIndex` / `DeleteIndex` / `ListIndexes`
- Extended `Query` with `filter` conditions
- `DefinePartitionSchema` / `GetSchema`

---

### Requirement 3: Filter Expressions on Query

#### Problem

Even without secondary indexes, the ability to filter query results by content fields server-side would be valuable. Currently all filtering happens in application code after fetching all items.

#### Proposed Capability

Add optional filter expressions to the existing `query` operation. Filters are applied after the partition key + sort key scan, before returning results.

```rust
// Current: fetch all contacts, filter in application
let all = db.query("memories").partition_key("contacts").execute()?;
let matches: Vec<_> = all.items.iter().filter(|item| item["email"] == "toby@example.com").collect();

// Proposed: filter at the database level
let matches = db.query("memories")
    .partition_key("contacts")
    .filter("email", "=", "toby@example.com")
    .execute()?;
```

#### Relationship to Indexes

- **Without an index:** Filter is a post-scan filter (reads all items in the partition, discards non-matches). Still useful — saves serialization and transfer overhead in server mode.
- **With an index:** Filter uses the index for an efficient lookup (reads only matching items). Major performance improvement.

The API should be the same either way. The database decides internally whether to use an index or fall back to scan+filter.

#### Filter Operations Needed

| Operator | Applies To | Example |
|---|---|---|
| `=` (equals) | Scalar fields | `email = "toby@example.com"` |
| `contains` | Array fields | `projects contains "ferridyndb"` |
| `exists` | Any field | `email exists` (field is present and non-null) |
| `begins_with` | String fields | `email begins_with "toby"` |

These four operators cover our use cases. We don't need `>`, `<`, `between`, `not`, or boolean combinators (`AND`/`OR`) for now.

---

## Priority and Phasing

We recognize this is substantial work. Here's how we'd phase adoption:

### Phase 1: Filter Expressions (Minimum Viable)

Add `filter` to the existing `query` operation as a post-scan filter. No indexes, no schema enforcement. This alone lets us stop doing application-layer filtering and moves the logic into the database.

**Plugin impact:** Update `recall` MCP tool to use filter expressions. No schema changes.

### Phase 2: Content Schemas

Add per-partition content schemas with validation on write. This gives us structural guarantees and prepares the ground for indexes.

**Plugin impact:** Migrate schema system from plugin-layer `_schema` meta-entries to native FerridynDB schemas. Schema inference via Haiku writes native DB schemas instead of custom metadata.

### Phase 3: Secondary Indexes

Add index creation and index-backed query execution. This is the big payoff — multi-path recall becomes a database operation instead of an application-layer hack.

**Plugin impact:** Define indexes on key content fields (email, project, tags). Remove NL resolution for queries that match indexed fields. Keep NL resolution only for genuinely fuzzy queries.

---

## Non-Requirements

Things we explicitly do NOT need (to keep scope bounded):

| Feature | Why Not |
|---|---|
| **Full-text search** | LLM-based NL resolution handles semantic/fuzzy queries |
| **Vector embeddings** | Same — LLM resolution is sufficient at our data volumes (hundreds to low thousands of items) |
| **Transactions** | Single-writer model; no concurrent conflict scenarios |
| **Batch write** | Write volume is low enough that individual put_item calls are fine |
| **TTL / expiration** | Memory expiration is an application concern, not a database concern |
| **Change streams / triggers** | No reactive patterns needed |
| **Cross-table joins** | Single-table design; no relational queries |
| **Schema migrations** | Application handles migrating data shapes when schemas evolve |

---

## Impact on ferridyn-memory Plugin

### Before (Current Architecture)

```
Agent writes "remember contacts toby ..."
  → Plugin validates sort key format via Haiku-inferred schema (stored as _schema entries)
  → Plugin stores content as opaque string
  → Database has no awareness of content structure

Agent recalls "who works on ferridyndb?"
  → Plugin calls Haiku to resolve NL → category=contacts (guess)
  → Plugin fetches ALL items in contacts partition
  → Plugin returns results (hoping the right ones are there)
  → If Haiku guesses wrong category, recall fails silently
```

### After (With These Features)

```
Agent writes "remember contacts toby ..."
  → Database validates content against contacts partition schema
  → Database indexes email, projects, tags fields automatically
  → Content is structured JSON, deterministically queryable

Agent recalls "who works on ferridyndb?"
  → Plugin resolves to: query contacts where projects contains "ferridyndb"
  → Database returns exact matches via index
  → Deterministic, fast, correct

Agent recalls by email "toby@example.com"
  → query contacts where email = "toby@example.com"
  → Direct index hit, single result

Agent recalls vague query "that person from last week"
  → No index can help → fall back to Haiku NL resolution (still available)
```

### Metrics That Improve

| Metric | Before | After |
|---|---|---|
| Recall precision (known-attribute queries) | ~80% (depends on Haiku guessing) | ~100% (deterministic index lookup) |
| Recall latency (indexed field) | 500-2000ms (Haiku API call) | <1ms (local index lookup) |
| Write validation | Non-deterministic (Haiku) | Deterministic (schema) |
| Cost per recall | ~$0.001 (Haiku call for NL resolution) | $0 for indexed queries, Haiku only for fuzzy |
| Cross-entity queries | Not possible without full scan | Native via array field indexes |

---

## Open Questions for FerridynDB Team

1. **Per-partition vs per-table schemas** — Is per-partition-key schema granularity feasible in the storage engine, or should we use separate tables per category? We strongly prefer per-partition to keep a unified query surface, but understand if the implementation cost is high.

2. **Index storage model** — Are indexes stored as separate internal tables/B-trees, or as inline metadata? This affects whether cross-partition indexes are cheap or expensive.

3. **Schema format** — Is JSON Schema the right format, or would a simpler custom format (like a field name → type map) be more practical for the storage engine?

4. **Filter without index performance** — For small partitions (<100 items), post-scan filtering is fine. Is there a partition size threshold where unindexed filters become problematic?

5. **Array field indexing** — Indexing individual elements of array fields (for `contains` queries) is important for our tags/projects use case. Is this a significant implementation complexity beyond scalar field indexing?

---

## Appendix: Example Data Shapes

### contacts partition
```json
{
  "category": "contacts",
  "key": "toby",
  "content": {
    "name": "Toby",
    "role": "backend engineer",
    "team": "platform",
    "email": "toby@example.com",
    "github": "@tobyx",
    "projects": ["ferridyndb", "ferridyn-memory"],
    "notes": "Prefers Rust. PST timezone."
  }
}
```
**Indexes needed:** `email` (exact), `projects` (contains), `team` (exact)

### decisions partition
```json
{
  "category": "decisions",
  "key": "auth#method",
  "content": {
    "decision": "JWT with refresh tokens",
    "rationale": "Stateless auth for microservices, refresh tokens for session management",
    "alternatives": ["server-side sessions", "OAuth2 only"],
    "area": "authentication",
    "date": "2026-01-15",
    "status": "active"
  }
}
```
**Indexes needed:** `area` (exact), `status` (exact)

### projects partition
```json
{
  "category": "projects",
  "key": "ferridyn-memory",
  "content": {
    "name": "ferridyn-memory",
    "description": "Claude Code memory plugin backed by FerridynDB",
    "tech_stack": ["rust", "typescript", "mcp"],
    "team": ["travis", "toby"],
    "status": "active",
    "repo": "https://github.com/AetherXHub/ferridyn-memory"
  }
}
```
**Indexes needed:** `tech_stack` (contains), `team` (contains), `status` (exact)
