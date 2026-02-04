//! Schema management for memory categories using native FerridynDB partition schemas.
//!
//! This module provides:
//! - [`SchemaManager`] for creating and querying partition schemas and secondary indexes
//! - [`InferredSchema`] for LLM-based schema inference on first write
//! - [`ResolvedQuery`] for routing natural language queries to the most efficient query strategy
//! - LLM-powered functions for schema inference, document parsing, and query resolution

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::warn;

use crate::backend::MemoryBackend;
use crate::error::MemoryError;
use crate::llm::{LlmClient, LlmError};

// Re-export server types used in public API.
pub use ferridyn_server::client::{
    AttributeDefInput, AttributeInfo, IndexInfo, PartitionSchemaInfo,
};

// ============================================================================
// Types
// ============================================================================

/// Schema inferred by Haiku on first write to a new category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InferredSchema {
    /// Human-readable description of the category.
    pub description: String,
    /// Typed attributes for items in this category.
    pub attributes: Vec<AttributeDef>,
    /// Attribute names that should be indexed for fast lookups.
    pub suggested_indexes: Vec<String>,
}

/// Attribute definition for schema inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeDef {
    pub name: String,
    /// One of "STRING", "NUMBER", "BOOLEAN".
    #[serde(rename = "type")]
    pub attr_type: String,
    pub required: bool,
}

/// Result of resolving a natural language query.
#[derive(Debug, Clone)]
pub enum ResolvedQuery {
    /// Use a secondary index for exact attribute lookup.
    IndexLookup {
        category: String,
        index_name: String,
        key_value: String,
    },
    /// Scan the partition with optional key prefix.
    PartitionScan {
        category: String,
        key_prefix: Option<String>,
    },
    /// Exact item by category + key.
    ExactLookup { category: String, key: String },
}

/// Result of classifying a natural language input's intent.
#[derive(Debug, Clone)]
pub enum NlIntent {
    /// User wants to store information. `content` has the command verb stripped.
    Remember { content: String },
    /// User wants to retrieve information.
    Recall { query: String },
}

// ============================================================================
// SchemaManager
// ============================================================================

/// Manages partition schemas and secondary indexes via the memory backend.
///
/// Delegates to native FerridynDB partition schema and index operations.
#[derive(Clone)]
pub struct SchemaManager {
    backend: MemoryBackend,
}

impl SchemaManager {
    pub fn new(backend: MemoryBackend) -> Self {
        Self { backend }
    }

    /// Check if a partition schema exists for a category.
    pub async fn has_schema(&self, category: &str) -> Result<bool, MemoryError> {
        match self.backend.describe_schema(category).await {
            Ok(_) => Ok(true),
            Err(MemoryError::Schema(ref msg))
                if msg.contains("not found")
                    || msg.contains("NotFound")
                    || msg.contains("does not exist")
                    || msg.contains("SchemaNotFound") =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }

    /// Get the partition schema for a category, if one exists.
    pub async fn get_schema(
        &self,
        category: &str,
    ) -> Result<Option<PartitionSchemaInfo>, MemoryError> {
        match self.backend.describe_schema(category).await {
            Ok(info) => Ok(Some(info)),
            Err(MemoryError::Schema(ref msg))
                if msg.contains("not found")
                    || msg.contains("NotFound")
                    || msg.contains("does not exist")
                    || msg.contains("SchemaNotFound") =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }

    /// List all partition schemas.
    pub async fn list_schemas(&self) -> Result<Vec<PartitionSchemaInfo>, MemoryError> {
        self.backend.list_schemas().await
    }

    /// Create a partition schema and secondary indexes from an inferred schema.
    ///
    /// When `validate` is true, the server will reject writes that don't conform
    /// to the schema. Use false for auto-inferred schemas (best-effort).
    pub async fn create_schema_with_indexes(
        &self,
        category: &str,
        inferred: &InferredSchema,
        validate: bool,
    ) -> Result<(), MemoryError> {
        let attrs: Vec<AttributeDefInput> = inferred
            .attributes
            .iter()
            .map(|a| AttributeDefInput {
                name: a.name.clone(),
                attr_type: a.attr_type.clone(),
                required: a.required,
            })
            .collect();

        self.backend
            .create_schema(category, Some(&inferred.description), &attrs, validate)
            .await?;

        // Create indexes for suggested attributes.
        for attr_name in &inferred.suggested_indexes {
            if let Some(attr) = inferred.attributes.iter().find(|a| &a.name == attr_name) {
                let index_name = format!("{category}_{attr_name}");
                if let Err(e) = self
                    .backend
                    .create_index(&index_name, category, attr_name, &attr.attr_type)
                    .await
                {
                    warn!("Failed to create index {index_name}: {e}");
                }
            }
        }

        Ok(())
    }

    /// List all secondary indexes.
    pub async fn list_indexes(&self) -> Result<Vec<IndexInfo>, MemoryError> {
        self.backend.list_indexes().await
    }

    /// Find a secondary index for a specific category and attribute.
    pub async fn find_index(
        &self,
        category: &str,
        attribute: &str,
    ) -> Result<Option<IndexInfo>, MemoryError> {
        let expected_name = format!("{category}_{attribute}");
        let indexes = self.backend.list_indexes().await?;
        Ok(indexes.into_iter().find(|idx| idx.name == expected_name))
    }
}

// ============================================================================
// LLM-Powered Schema Inference
// ============================================================================

const INFER_SCHEMA_PROMPT: &str = r#"You are a schema inference engine for a structured memory system. Given a category name and natural language input about what will be stored, infer the schema.

Respond with ONLY a JSON object (no markdown, no explanation):
{
  "description": "Human-readable description of what this category stores",
  "attributes": [
    {"name": "attribute_name", "type": "STRING", "required": true},
    ...
  ],
  "suggested_indexes": ["attribute_name_worth_indexing", ...]
}

Rules:
- Attribute types must be one of: STRING, NUMBER, BOOLEAN
- Mark attributes as required only if they will ALWAYS be present in every item
- Suggest indexes for attributes commonly used in lookups (e.g. email, name)
- Keep attribute names lowercase with underscores
- Include 3-6 relevant DOMAIN attributes based on the category and input
- Do NOT include "category", "key", or any metadata attributes — those are handled automatically
- Focus on content attributes (e.g. for appointments: date, time, location, description, doctor_name)
- Do NOT include attributes like "category_name", "category_type", "item_type" — those are system-level"#;

/// Infer a schema from the first write to a category.
///
/// Returns `None` if inference fails (never blocks writes).
pub async fn infer_schema(
    llm: &dyn LlmClient,
    category: &str,
    input: &str,
) -> Option<InferredSchema> {
    let user_msg = format!("Category: {category}\nInput: {input}");

    match llm.complete(INFER_SCHEMA_PROMPT, &user_msg).await {
        Ok(completion) => {
            let cleaned = strip_markdown_fences(completion.text.trim());
            match serde_json::from_str::<InferredSchema>(&cleaned) {
                Ok(schema) => Some(schema),
                Err(e) => {
                    warn!("Failed to parse inferred schema: {e}");
                    None
                }
            }
        }
        Err(e) => {
            warn!("Schema inference LLM call failed: {e}");
            None
        }
    }
}

// ============================================================================
// LLM-Powered Document Parsing
// ============================================================================

const PARSE_DOCUMENT_PROMPT: &str = r#"You are a document parser for a structured memory system. Given a category schema and natural language input, extract a structured JSON document.

Respond with ONLY a JSON object (no markdown, no explanation):
{
  "key": "short-identifier-for-this-item",
  "attribute1": "value1",
  "attribute2": "value2",
  ...
}

Rules:
- "key" must be a short, lowercase, hyphenated identifier (e.g. "toby", "auth-method", "ferridyndb")
- Extract values for each schema attribute from the input text
- Use null for attributes not mentioned in the input
- For STRING attributes: use plain text values
- For NUMBER attributes: use numeric values
- For BOOLEAN attributes: use true/false
- Keep values concise but complete
- IMPORTANT: Resolve all relative dates and times to absolute values using the provided current date. "tomorrow" → actual date, "next week" → actual date, "in 3 days" → actual date. Use ISO 8601 format (YYYY-MM-DD) for dates and 24h format (HH:MM) for times."#;

/// Parse natural language input into a structured document using the schema.
pub async fn parse_to_document(
    llm: &dyn LlmClient,
    category: &str,
    schema: &PartitionSchemaInfo,
    input: &str,
) -> Result<Value, LlmError> {
    let attrs_desc: Vec<String> = schema
        .attributes
        .iter()
        .map(|a| {
            format!(
                "  - {} ({}{})",
                a.name,
                a.attr_type,
                if a.required { ", required" } else { "" }
            )
        })
        .collect();

    let today = chrono::Local::now().format("%Y-%m-%d (%A)");
    let user_msg = format!(
        "Today's date: {today}\nCategory: {category}\nSchema description: {}\nAttributes:\n{}\n\nInput: {input}",
        schema.description,
        attrs_desc.join("\n")
    );

    let completion = llm.complete(PARSE_DOCUMENT_PROMPT, &user_msg).await?;
    let cleaned = strip_markdown_fences(completion.text.trim());

    serde_json::from_str(&cleaned).map_err(|e| {
        LlmError::Parse(format!(
            "Failed to parse document: {e}\nResponse: {}",
            completion.text
        ))
    })
}

// ============================================================================
// LLM-Powered Query Resolution
// ============================================================================

const RESOLVE_QUERY_PROMPT: &str = r#"You are a query resolver for a structured memory system. Given the available schemas, indexes, existing keys, and a natural language query, determine how to find the data.

Respond with ONLY a JSON object (no markdown, no explanation). Use one of these forms:

For exact item lookup (when the query maps to a known key):
{"type": "exact", "category": "name", "key": "item-key"}

For partition scan with begins_with prefix (to narrow results by key prefix):
{"type": "scan", "category": "name", "key_prefix": "prefix"}

For full category scan (when you need all items):
{"type": "scan", "category": "name", "key_prefix": null}

For index-based lookup (when query targets a specific indexed attribute value you KNOW):
{"type": "index", "category": "name", "index_name": "category_attribute", "key_value": "exact_value"}

Rules:
- You are given the EXISTING KEYS for each category — use them to pick the best strategy
- If a known key matches the query, use exact lookup (e.g. query "doctor appointment" + key "doctor-appointment" → exact)
- If part of the query matches the START of known keys, use scan with key_prefix (begins_with match)
- key_prefix does a begins_with match on sort keys — "doctor" matches "doctor-appointment", "doctor-checkup", etc.
- Use null key_prefix only when you need ALL items in a category
- Only use index lookup for specific attribute VALUE queries (e.g. "who has email toby@example.com")
- Choose the category that best matches what the user is asking about"#;

/// Resolve a natural language query to a [`ResolvedQuery`].
///
/// `category_keys` maps each category name to its existing sort keys (up to a sample limit).
/// This helps the LLM match queries to concrete keys and prefixes.
pub async fn resolve_query(
    llm: &dyn LlmClient,
    schemas: &[PartitionSchemaInfo],
    indexes: &[IndexInfo],
    category_keys: &[(String, Vec<String>)],
    query: &str,
) -> Result<ResolvedQuery, LlmError> {
    let mut schema_desc = String::new();
    for schema in schemas {
        let keys_for_cat: Vec<&str> = category_keys
            .iter()
            .find(|(cat, _)| cat == &schema.prefix)
            .map(|(_, keys)| keys.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        let keys_str = if keys_for_cat.is_empty() {
            "(empty)".to_string()
        } else {
            keys_for_cat.join(", ")
        };

        schema_desc.push_str(&format!(
            "\nCategory: {}\n  Description: {}\n  Attributes: {}\n  Keys: {}\n",
            schema.prefix,
            schema.description,
            schema
                .attributes
                .iter()
                .map(|a| format!("{}({})", a.name, a.attr_type))
                .collect::<Vec<_>>()
                .join(", "),
            keys_str,
        ));
    }

    let mut index_desc = String::new();
    if indexes.is_empty() {
        index_desc.push_str("\n(none)");
    } else {
        for idx in indexes {
            index_desc.push_str(&format!(
                "\nIndex: {} (category={}, attribute={}, type={})",
                idx.name, idx.partition_schema, idx.index_key_name, idx.index_key_type
            ));
        }
    }

    let today = chrono::Local::now().format("%Y-%m-%d (%A)");
    let user_msg = format!(
        "Today's date: {today}\n\nAvailable schemas:{schema_desc}\nAvailable indexes:{index_desc}\n\nQuery: {query}"
    );

    let completion = llm.complete(RESOLVE_QUERY_PROMPT, &user_msg).await?;
    let cleaned = strip_markdown_fences(completion.text.trim());

    let parsed: Value = serde_json::from_str(&cleaned).map_err(|e| {
        LlmError::Parse(format!(
            "Failed to parse resolve response: {e}\nResponse: {}",
            completion.text
        ))
    })?;

    let query_type = parsed["type"]
        .as_str()
        .ok_or_else(|| LlmError::Parse("Missing 'type' in resolve response".into()))?;

    match query_type {
        "index" => {
            let category = parsed["category"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'category' in index lookup".into()))?
                .to_string();
            let index_name = parsed["index_name"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'index_name' in index lookup".into()))?
                .to_string();
            let key_value = parsed["key_value"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'key_value' in index lookup".into()))?
                .to_string();
            Ok(ResolvedQuery::IndexLookup {
                category,
                index_name,
                key_value,
            })
        }
        "scan" => {
            let category = parsed["category"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'category' in scan".into()))?
                .to_string();
            let key_prefix = parsed["key_prefix"].as_str().map(|s| s.to_string());
            Ok(ResolvedQuery::PartitionScan {
                category,
                key_prefix,
            })
        }
        "exact" => {
            let category = parsed["category"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'category' in exact lookup".into()))?
                .to_string();
            let key = parsed["key"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'key' in exact lookup".into()))?
                .to_string();
            Ok(ResolvedQuery::ExactLookup { category, key })
        }
        other => Err(LlmError::Parse(format!(
            "Unknown query type: {other}. Expected 'index', 'scan', or 'exact'"
        ))),
    }
}

// ============================================================================
// LLM-Powered Intent Classification
// ============================================================================

const CLASSIFY_INTENT_PROMPT: &str = r#"You are an intent classifier for a memory system. Given natural language input, determine if the user wants to STORE a new memory or RECALL an existing one.

Respond with ONLY a JSON object (no markdown, no explanation):

For storing: {"intent": "remember", "content": "the cleaned information to store"}
For recalling: {"intent": "recall", "query": "the search query"}

Rules:
- Complete sentences that state facts → STORE (e.g. "my favorite food is ramen", "Toby works at Acme", "the API uses JWT auth")
- Sentences with "remember", "store", "save", "note that" → STORE. Strip the command verb from content.
- "remember I ..." or "I ..." statements → STORE
- Questions (what, who, when, where, how) → RECALL
- Imperative retrieval ("show me", "find", "get", "list", "tell me") → RECALL
- Short noun phrases seeking information → RECALL (e.g. "Toby's email", "API endpoints")
- Key distinction: if the input PROVIDES information, it's STORE. If it SEEKS information, it's RECALL.
- Default to STORE if ambiguous — it's safer to store than to lose information"#;

/// Classify a natural language input as either a remember (store) or recall (retrieve) intent.
pub async fn classify_intent(llm: &dyn LlmClient, input: &str) -> Result<NlIntent, LlmError> {
    let completion = llm.complete(CLASSIFY_INTENT_PROMPT, input).await?;
    let cleaned = strip_markdown_fences(completion.text.trim());

    let parsed: Value = serde_json::from_str(&cleaned).map_err(|e| {
        LlmError::Parse(format!(
            "Failed to parse intent classification: {e}\nResponse: {}",
            completion.text
        ))
    })?;

    let intent = parsed["intent"]
        .as_str()
        .ok_or_else(|| LlmError::Parse("Missing 'intent' in classification response".into()))?;

    match intent {
        "remember" => {
            let content = parsed["content"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'content' in remember intent".into()))?
                .to_string();
            Ok(NlIntent::Remember { content })
        }
        "recall" => {
            let query = parsed["query"]
                .as_str()
                .ok_or_else(|| LlmError::Parse("Missing 'query' in recall intent".into()))?
                .to_string();
            Ok(NlIntent::Recall { query })
        }
        other => Err(LlmError::Parse(format!(
            "Unknown intent: {other}. Expected 'remember' or 'recall'"
        ))),
    }
}

// ============================================================================
// LLM-Powered Answer Synthesis
// ============================================================================

const ANSWER_QUERY_PROMPT: &str = r#"You are answering a question using data from a personal memory system. Given the user's question and retrieved memory items, provide a concise, direct answer.

Rules:
- Answer the question directly using ONLY the data provided
- If the data contains the answer, state it clearly in 1-3 sentences
- If the data doesn't directly answer the question but has related information, summarize what's relevant
- If no items are relevant at all, respond with exactly: NO_RELEVANT_DATA
- Do NOT add speculation, caveats, or information not present in the data
- Do NOT mention "the data shows" or "according to the records" — just answer naturally
- For dates and times, state them clearly (e.g. "Your doctor's appointment is on 2026-02-03 at 12:00")"#;

/// Synthesize a natural language answer from retrieved items and the original query.
///
/// Returns `None` if the LLM determines no items are relevant.
pub async fn answer_query(
    llm: &dyn LlmClient,
    query: &str,
    items: &[Value],
) -> Result<Option<String>, LlmError> {
    let items_json = serde_json::to_string_pretty(items).unwrap_or_default();
    let today = chrono::Local::now().format("%Y-%m-%d (%A)");

    let user_msg =
        format!("Today's date: {today}\n\nQuestion: {query}\n\nRetrieved items:\n{items_json}");

    let completion = llm.complete(ANSWER_QUERY_PROMPT, &user_msg).await?;
    let text = completion.text.trim().to_string();

    if text == "NO_RELEVANT_DATA" {
        Ok(None)
    } else {
        Ok(Some(text))
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Strip markdown code fences from LLM output.
pub fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with("```") {
        let after_first_fence = trimmed
            .find('\n')
            .map(|i| &trimmed[i + 1..])
            .unwrap_or(trimmed);
        if let Some(end) = after_first_fence.rfind("```") {
            return after_first_fence[..end].trim().to_string();
        }
    }
    trimmed.to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmClient;

    // --- strip_markdown_fences ---

    #[test]
    fn test_strip_no_fences() {
        assert_eq!(strip_markdown_fences("hello"), "hello");
    }

    #[test]
    fn test_strip_json_fences() {
        assert_eq!(strip_markdown_fences("```json\n{}\n```"), "{}");
    }

    #[test]
    fn test_strip_bare_fences() {
        assert_eq!(strip_markdown_fences("```\nfoo\n```"), "foo");
    }

    // --- infer_schema ---

    #[tokio::test]
    async fn test_infer_schema_success() {
        let mock = MockLlmClient::new(vec![
            r#"{"description":"People and contacts","attributes":[{"name":"name","type":"STRING","required":true},{"name":"email","type":"STRING","required":true},{"name":"role","type":"STRING","required":false}],"suggested_indexes":["email"]}"#.into(),
        ]);

        let result = infer_schema(
            &mock,
            "contacts",
            "Toby is a backend engineer, email toby@example.com",
        )
        .await;
        let schema = result.unwrap();
        assert_eq!(schema.description, "People and contacts");
        assert_eq!(schema.attributes.len(), 3);
        assert_eq!(schema.suggested_indexes, vec!["email"]);
    }

    #[tokio::test]
    async fn test_infer_schema_bad_json_returns_none() {
        let mock = MockLlmClient::new(vec!["not valid json".into()]);
        let result = infer_schema(&mock, "contacts", "some input").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_infer_schema_with_markdown_fences() {
        let mock = MockLlmClient::new(vec![
            "```json\n{\"description\":\"People\",\"attributes\":[{\"name\":\"name\",\"type\":\"STRING\",\"required\":true}],\"suggested_indexes\":[\"name\"]}\n```".into(),
        ]);

        let result = infer_schema(&mock, "contacts", "Toby").await;
        assert!(result.is_some());
    }

    // --- parse_to_document ---

    #[tokio::test]
    async fn test_parse_to_document_success() {
        let mock = MockLlmClient::new(vec![
            r#"{"key":"toby","name":"Toby","email":"toby@example.com","role":"backend engineer"}"#
                .into(),
        ]);

        let schema = PartitionSchemaInfo {
            prefix: "contacts".into(),
            description: "People and contacts".into(),
            attributes: vec![
                AttributeInfo {
                    name: "name".into(),
                    attr_type: "STRING".into(),
                    required: true,
                },
                AttributeInfo {
                    name: "email".into(),
                    attr_type: "STRING".into(),
                    required: true,
                },
                AttributeInfo {
                    name: "role".into(),
                    attr_type: "STRING".into(),
                    required: false,
                },
            ],
            validate: true,
        };

        let doc = parse_to_document(
            &mock,
            "contacts",
            &schema,
            "Toby is a backend engineer, email toby@example.com",
        )
        .await
        .unwrap();
        assert_eq!(doc["key"], "toby");
        assert_eq!(doc["name"], "Toby");
        assert_eq!(doc["email"], "toby@example.com");
    }

    #[tokio::test]
    async fn test_parse_to_document_with_fences() {
        let mock = MockLlmClient::new(vec![
            "```json\n{\"key\":\"toby\",\"name\":\"Toby\"}\n```".into(),
        ]);

        let schema = PartitionSchemaInfo {
            prefix: "contacts".into(),
            description: "People".into(),
            attributes: vec![AttributeInfo {
                name: "name".into(),
                attr_type: "STRING".into(),
                required: true,
            }],
            validate: true,
        };

        let doc = parse_to_document(&mock, "contacts", &schema, "Toby")
            .await
            .unwrap();
        assert_eq!(doc["key"], "toby");
    }

    // --- resolve_query ---

    #[tokio::test]
    async fn test_resolve_query_index_lookup() {
        let mock = MockLlmClient::new(vec![
            r#"{"type":"index","category":"contacts","index_name":"contacts_email","key_value":"toby@example.com"}"#.into(),
        ]);

        let schemas = vec![PartitionSchemaInfo {
            prefix: "contacts".into(),
            description: "People".into(),
            attributes: vec![AttributeInfo {
                name: "email".into(),
                attr_type: "STRING".into(),
                required: true,
            }],
            validate: true,
        }];
        let indexes = vec![IndexInfo {
            name: "contacts_email".into(),
            partition_schema: "contacts".into(),
            index_key_name: "email".into(),
            index_key_type: "STRING".into(),
        }];

        let result = resolve_query(&mock, &schemas, &indexes, &[], "Toby's email")
            .await
            .unwrap();
        match result {
            ResolvedQuery::IndexLookup {
                category,
                index_name,
                key_value,
            } => {
                assert_eq!(category, "contacts");
                assert_eq!(index_name, "contacts_email");
                assert_eq!(key_value, "toby@example.com");
            }
            _ => panic!("Expected IndexLookup"),
        }
    }

    #[tokio::test]
    async fn test_resolve_query_partition_scan() {
        let mock = MockLlmClient::new(vec![
            r#"{"type":"scan","category":"decisions","key_prefix":null}"#.into(),
        ]);

        let schemas = vec![PartitionSchemaInfo {
            prefix: "decisions".into(),
            description: "Decisions".into(),
            attributes: vec![],
            validate: false,
        }];

        let result = resolve_query(&mock, &schemas, &[], &[], "all decisions")
            .await
            .unwrap();
        match result {
            ResolvedQuery::PartitionScan {
                category,
                key_prefix,
            } => {
                assert_eq!(category, "decisions");
                assert!(key_prefix.is_none());
            }
            _ => panic!("Expected PartitionScan"),
        }
    }

    #[tokio::test]
    async fn test_resolve_query_exact_lookup() {
        let mock = MockLlmClient::new(vec![
            r#"{"type":"exact","category":"contacts","key":"toby"}"#.into(),
        ]);

        let schemas = vec![PartitionSchemaInfo {
            prefix: "contacts".into(),
            description: "People".into(),
            attributes: vec![],
            validate: false,
        }];

        let result = resolve_query(&mock, &schemas, &[], &[], "get toby's contact info")
            .await
            .unwrap();
        match result {
            ResolvedQuery::ExactLookup { category, key } => {
                assert_eq!(category, "contacts");
                assert_eq!(key, "toby");
            }
            _ => panic!("Expected ExactLookup"),
        }
    }

    #[tokio::test]
    async fn test_resolve_query_with_markdown_fences() {
        let mock = MockLlmClient::new(vec![
            "```json\n{\"type\":\"scan\",\"category\":\"contacts\",\"key_prefix\":\"toby\"}\n```"
                .into(),
        ]);

        let schemas = vec![PartitionSchemaInfo {
            prefix: "contacts".into(),
            description: "People".into(),
            attributes: vec![],
            validate: false,
        }];

        let result = resolve_query(&mock, &schemas, &[], &[], "toby")
            .await
            .unwrap();
        match result {
            ResolvedQuery::PartitionScan {
                category,
                key_prefix,
            } => {
                assert_eq!(category, "contacts");
                assert_eq!(key_prefix.unwrap(), "toby");
            }
            _ => panic!("Expected PartitionScan"),
        }
    }

    // --- classify_intent ---

    #[tokio::test]
    async fn test_classify_intent_remember() {
        let mock = MockLlmClient::new(vec![
            r#"{"intent":"remember","content":"I have an appointment at noon tomorrow"}"#.into(),
        ]);

        let result = classify_intent(&mock, "remember I have an appointment at noon tomorrow")
            .await
            .unwrap();
        match result {
            NlIntent::Remember { content } => {
                assert_eq!(content, "I have an appointment at noon tomorrow");
            }
            _ => panic!("Expected Remember intent"),
        }
    }

    #[tokio::test]
    async fn test_classify_intent_recall() {
        let mock = MockLlmClient::new(vec![
            r#"{"intent":"recall","query":"what is Toby's email"}"#.into(),
        ]);

        let result = classify_intent(&mock, "what is Toby's email")
            .await
            .unwrap();
        match result {
            NlIntent::Recall { query } => {
                assert_eq!(query, "what is Toby's email");
            }
            _ => panic!("Expected Recall intent"),
        }
    }

    #[tokio::test]
    async fn test_classify_intent_with_fences() {
        let mock = MockLlmClient::new(vec![
            "```json\n{\"intent\":\"remember\",\"content\":\"Toby is a backend engineer\"}\n```"
                .into(),
        ]);

        let result = classify_intent(&mock, "remember Toby is a backend engineer")
            .await
            .unwrap();
        match result {
            NlIntent::Remember { content } => {
                assert_eq!(content, "Toby is a backend engineer");
            }
            _ => panic!("Expected Remember intent"),
        }
    }

    // --- answer_query ---

    #[tokio::test]
    async fn test_answer_query_returns_answer() {
        let mock = MockLlmClient::new(vec![
            "Your doctor's appointment is on 2026-02-03 at 12:00.".into(),
        ]);

        let items = vec![serde_json::json!({
            "category": "appointment",
            "key": "doctor-appointment",
            "date": "2026-02-03",
            "time": "12:00",
            "title": "Doctor's Appointment",
        })];

        let result = answer_query(&mock, "when is my doctors appointment", &items)
            .await
            .unwrap();
        assert!(result.is_some());
        assert!(result.unwrap().contains("12:00"));
    }

    #[tokio::test]
    async fn test_answer_query_no_relevant_data() {
        let mock = MockLlmClient::new(vec!["NO_RELEVANT_DATA".into()]);

        let items = vec![serde_json::json!({
            "category": "preference",
            "key": "food",
            "favorite": "ramen",
        })];

        let result = answer_query(&mock, "when is my doctors appointment", &items)
            .await
            .unwrap();
        assert!(result.is_none());
    }
}
