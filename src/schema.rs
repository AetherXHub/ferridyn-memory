//! Schema system for memory categories.
//!
//! Each category can have a [`CategorySchema`] that describes the expected
//! sort key format, its segments, and provides examples. Schemas are stored
//! in the `_schema` meta-category and used to:
//!
//! - **Validate** sort keys on writes (regex compiled from `sort_key_format`)
//! - **Infer** schemas automatically on first write via LLM
//! - **Resolve** natural language queries to `(category, prefix)` pairs

use std::collections::HashMap;
use std::sync::Arc;

use indexmap::IndexMap;
use regex::Regex;
use rmcp::ErrorData;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::warn;

use crate::backend::MemoryBackend;
use crate::llm::{LlmClient, LlmError};

// ============================================================================
// Constants
// ============================================================================

/// Meta-category used to store schema definitions.
pub const SCHEMA_CATEGORY: &str = "_schema";

// ============================================================================
// Schema Data Model
// ============================================================================

/// Schema definition for a memory category.
///
/// Describes the expected sort key format and provides documentation for
/// agents using the memory system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySchema {
    /// Human-readable description of what this category stores.
    pub description: String,

    /// Sort key format template, e.g. `"{name}#{attribute}"`.
    ///
    /// Segments are delimited by `#` and enclosed in `{}`.
    pub sort_key_format: String,

    /// Ordered map of segment name to description.
    ///
    /// Order matches the segment order in `sort_key_format`.
    pub segments: IndexMap<String, String>,

    /// Example sort keys demonstrating the expected format.
    pub examples: Vec<String>,
}

// ============================================================================
// Schema Store
// ============================================================================

/// Manages schema CRUD operations against the memory backend.
///
/// Schemas are stored as regular memory items in the `_schema` meta-category
/// with the target category name as the sort key and the JSON-serialized
/// [`CategorySchema`] as the content.
#[derive(Clone)]
pub struct SchemaStore {
    backend: MemoryBackend,
    /// Cache of compiled validation regexes, keyed by category name.
    regex_cache: Arc<RwLock<HashMap<String, Regex>>>,
}

impl SchemaStore {
    /// Create a new schema store backed by the given memory backend.
    pub fn new(backend: MemoryBackend) -> Self {
        Self {
            backend,
            regex_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Retrieve the schema for a category, if one exists.
    pub async fn get_schema(&self, category: &str) -> Result<Option<CategorySchema>, ErrorData> {
        let item = self.backend.get_item(SCHEMA_CATEGORY, category).await?;
        match item {
            Some(doc) => {
                let content = doc["content"]
                    .as_str()
                    .ok_or_else(|| ErrorData::internal_error("Schema missing content", None))?;
                let schema: CategorySchema = serde_json::from_str(content).map_err(|e| {
                    ErrorData::internal_error(format!("Schema parse error: {e}"), None)
                })?;
                Ok(Some(schema))
            }
            None => Ok(None),
        }
    }

    /// Store a schema for a category.
    pub async fn put_schema(
        &self,
        category: &str,
        schema: &CategorySchema,
    ) -> Result<(), ErrorData> {
        let content = serde_json::to_string(schema)
            .map_err(|e| ErrorData::internal_error(format!("Schema serialize error: {e}"), None))?;

        let doc = serde_json::json!({
            "category": SCHEMA_CATEGORY,
            "key": category,
            "content": content,
        });

        self.backend.put_item(doc).await?;

        // Invalidate cached regex for this category.
        self.regex_cache.write().await.remove(category);

        Ok(())
    }

    /// Check whether a category has a schema defined.
    pub async fn has_schema(&self, category: &str) -> Result<bool, ErrorData> {
        Ok(self.get_schema(category).await?.is_some())
    }

    /// List all defined schemas as `(category_name, schema)` pairs.
    pub async fn list_schemas(&self) -> Result<Vec<(String, CategorySchema)>, ErrorData> {
        let items = self.backend.query(SCHEMA_CATEGORY, None, 1000).await?;
        let mut schemas = Vec::new();

        for item in items {
            let cat = item["key"].as_str().unwrap_or_default().to_string();
            let content = item["content"].as_str().unwrap_or_default();
            if let Ok(schema) = serde_json::from_str::<CategorySchema>(content) {
                schemas.push((cat, schema));
            }
        }

        Ok(schemas)
    }

    /// Validate a sort key against the category's schema.
    ///
    /// Returns `Ok(())` if valid or if no schema exists.
    /// Returns `Err(message)` if the key doesn't match the expected format.
    pub async fn validate_key(&self, category: &str, key: &str) -> Result<(), String> {
        let schema = self
            .get_schema(category)
            .await
            .map_err(|e| format!("Failed to read schema: {}", e.message))?;

        let schema = match schema {
            Some(s) => s,
            None => return Ok(()), // No schema = no validation
        };

        let regex = self.get_or_compile_regex(category, &schema).await?;

        if regex.is_match(key) {
            Ok(())
        } else {
            let segment_desc: Vec<String> = schema
                .segments
                .iter()
                .map(|(name, desc)| format!("  {name}: {desc}"))
                .collect();
            let examples: Vec<String> = schema.examples.iter().map(|e| format!("  {e}")).collect();

            Err(format!(
                "Key \"{key}\" doesn't match expected format: {}\n\nSegments:\n{}\n\nExamples:\n{}",
                schema.sort_key_format,
                segment_desc.join("\n"),
                examples.join("\n"),
            ))
        }
    }

    /// Get a cached regex or compile one from the schema's sort_key_format.
    async fn get_or_compile_regex(
        &self,
        category: &str,
        schema: &CategorySchema,
    ) -> Result<Regex, String> {
        // Check cache first.
        {
            let cache = self.regex_cache.read().await;
            if let Some(re) = cache.get(category) {
                return Ok(re.clone());
            }
        }

        // Compile and cache.
        let re = compile_format_regex(&schema.sort_key_format)?;
        self.regex_cache
            .write()
            .await
            .insert(category.to_string(), re.clone());
        Ok(re)
    }
}

// ============================================================================
// Schema Inference
// ============================================================================

/// System prompt for schema inference.
const INFER_SYSTEM_PROMPT: &str = r#"You are a schema inference engine for a memory system. Given a category name, sort key, and content, infer the schema for this category.

Respond with ONLY a JSON object (no markdown, no explanation) matching this structure:
{
  "description": "Human-readable description of what this category stores",
  "sort_key_format": "Template like {segment1}#{segment2}",
  "segments": {"segment1": "description of segment1", "segment2": "description of segment2"},
  "examples": ["example-key1#attr1", "example-key2#attr2"]
}

Rules:
- The sort_key_format uses {name} placeholders separated by # delimiters
- Segment names should be descriptive (e.g. "person_name", "attribute", "topic")
- Provide 2-3 realistic example keys
- Keep descriptions concise
- segments must be an ordered object matching the placeholders in sort_key_format"#;

/// Infer a schema from the first write to a category.
///
/// Sends the category name, key, and content to the LLM and parses the
/// response as a [`CategorySchema`]. Returns `None` if inference fails
/// (never blocks writes).
pub async fn infer_schema(
    llm: &dyn LlmClient,
    category: &str,
    key: &str,
    content: &str,
) -> Option<CategorySchema> {
    let user_msg = format!("Category: {category}\nSort Key: {key}\nContent: {content}");

    match llm.complete(INFER_SYSTEM_PROMPT, &user_msg).await {
        Ok(completion) => match parse_schema_json(&completion.text) {
            Ok(schema) => {
                if let Err(e) = validate_schema_format(&schema) {
                    warn!("Inferred schema has invalid format: {e}");
                    None
                } else {
                    Some(schema)
                }
            }
            Err(e) => {
                warn!("Failed to parse inferred schema: {e}");
                None
            }
        },
        Err(e) => {
            warn!("Schema inference LLM call failed: {e}");
            None
        }
    }
}

// ============================================================================
// Natural Language Query Resolution
// ============================================================================

/// System prompt for resolving natural language queries.
const RESOLVE_SYSTEM_PROMPT: &str = r#"You are a query resolver for a memory system. Given the available schemas and a natural language query, determine which category and sort key prefix to search.

Respond with ONLY a JSON object (no markdown, no explanation):
{
  "category": "the category name to search",
  "prefix": "sort key prefix to filter by, or null for all entries"
}"#;

/// Resolve a natural language query to a (category, optional prefix) pair.
///
/// Sends all known schemas plus the query to the LLM and parses the response.
pub async fn resolve_query(
    llm: &dyn LlmClient,
    schemas: &[(String, CategorySchema)],
    query: &str,
) -> Result<(String, Option<String>), LlmError> {
    let mut schema_descriptions = String::new();
    for (name, schema) in schemas {
        schema_descriptions.push_str(&format!(
            "\nCategory: {name}\n  Description: {}\n  Key format: {}\n  Segments: {:?}\n  Examples: {:?}\n",
            schema.description, schema.sort_key_format, schema.segments, schema.examples
        ));
    }

    let user_msg = format!("Available schemas:\n{schema_descriptions}\nQuery: {query}");

    let completion = llm.complete(RESOLVE_SYSTEM_PROMPT, &user_msg).await?;

    let cleaned = strip_markdown_fences(completion.text.trim());
    let parsed: serde_json::Value = serde_json::from_str(&cleaned).map_err(|e| {
        LlmError::Parse(format!(
            "Failed to parse resolve response: {e}\nResponse: {}",
            completion.text
        ))
    })?;

    let category = parsed["category"]
        .as_str()
        .ok_or_else(|| LlmError::Parse("Missing 'category' in resolve response".into()))?
        .to_string();

    let prefix = parsed["prefix"].as_str().map(|s| s.to_string());

    Ok((category, prefix))
}

// ============================================================================
// Helpers
// ============================================================================

/// Compile a sort_key_format string into a validation regex.
///
/// Converts `{segment}` placeholders to `[^#]+` capture groups and `#`
/// delimiters to literal `#` separators.
///
/// Example: `"{name}#{attribute}"` → `^(?P<name>[^#]+)#(?P<attribute>[^#]+)$`
pub fn compile_format_regex(format: &str) -> Result<Regex, String> {
    let placeholder_re =
        Regex::new(r"\{(\w+)\}").map_err(|e| format!("Internal regex error: {e}"))?;

    let mut pattern = String::from("^");
    let mut last_end = 0;

    for cap in placeholder_re.captures_iter(format) {
        let full_match = cap.get(0).unwrap();
        let name = &cap[1];

        // Append any literal text between placeholders (escaped).
        let literal = &format[last_end..full_match.start()];
        pattern.push_str(&regex::escape(literal));

        // Append named capture group.
        pattern.push_str(&format!("(?P<{name}>[^#]+)"));
        last_end = full_match.end();
    }

    // Append any trailing literal.
    let trailing = &format[last_end..];
    pattern.push_str(&regex::escape(trailing));
    pattern.push('$');

    Regex::new(&pattern).map_err(|e| format!("Failed to compile format regex: {e}"))
}

/// Parse a JSON string into a CategorySchema, stripping markdown fences if present.
fn parse_schema_json(text: &str) -> Result<CategorySchema, String> {
    let cleaned = strip_markdown_fences(text);
    serde_json::from_str(&cleaned).map_err(|e| format!("JSON parse error: {e}"))
}

/// Strip markdown code fences from LLM output.
fn strip_markdown_fences(text: &str) -> String {
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

/// Validate that a schema's segments match its sort_key_format placeholders.
pub fn validate_schema_format(schema: &CategorySchema) -> Result<(), String> {
    let placeholder_re = Regex::new(r"\{(\w+)\}").unwrap();
    let placeholders: Vec<&str> = placeholder_re
        .captures_iter(&schema.sort_key_format)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();

    let segment_names: Vec<&str> = schema.segments.keys().map(|s| s.as_str()).collect();

    if placeholders != segment_names {
        return Err(format!(
            "Segment names {:?} don't match format placeholders {:?}",
            segment_names, placeholders
        ));
    }

    Ok(())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- compile_format_regex ---

    #[test]
    fn test_compile_single_segment() {
        let re = compile_format_regex("{name}").unwrap();
        assert!(re.is_match("toby"));
        assert!(re.is_match("alice-wonderland"));
        assert!(!re.is_match("")); // [^#]+ requires at least one char
    }

    #[test]
    fn test_compile_two_segments() {
        let re = compile_format_regex("{name}#{attribute}").unwrap();
        assert!(re.is_match("toby#email"));
        assert!(re.is_match("alice#phone"));
        assert!(!re.is_match("toby")); // missing #attribute
        assert!(!re.is_match("toby#")); // empty attribute
        assert!(!re.is_match("#email")); // empty name
    }

    #[test]
    fn test_compile_three_segments() {
        let re = compile_format_regex("{area}#{topic}#{subtopic}").unwrap();
        assert!(re.is_match("arch#patterns#singleton"));
        assert!(!re.is_match("arch#patterns")); // missing segment
    }

    #[test]
    fn test_named_captures() {
        let re = compile_format_regex("{name}#{attribute}").unwrap();
        let caps = re.captures("toby#email").unwrap();
        assert_eq!(&caps["name"], "toby");
        assert_eq!(&caps["attribute"], "email");
    }

    // --- validate_schema_format ---

    #[test]
    fn test_validate_matching_segments() {
        let schema = CategorySchema {
            description: "test".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: IndexMap::from([
                ("name".into(), "person name".into()),
                ("attribute".into(), "contact attribute".into()),
            ]),
            examples: vec!["toby#email".into()],
        };
        assert!(validate_schema_format(&schema).is_ok());
    }

    #[test]
    fn test_validate_mismatched_segments() {
        let schema = CategorySchema {
            description: "test".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: IndexMap::from([("name".into(), "person name".into())]),
            examples: vec![],
        };
        assert!(validate_schema_format(&schema).is_err());
    }

    #[test]
    fn test_validate_wrong_order() {
        let schema = CategorySchema {
            description: "test".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: IndexMap::from([
                ("attribute".into(), "contact attribute".into()),
                ("name".into(), "person name".into()),
            ]),
            examples: vec![],
        };
        assert!(validate_schema_format(&schema).is_err());
    }

    // --- parse_schema_json ---

    #[test]
    fn test_parse_clean_json() {
        let json = r#"{"description":"People","sort_key_format":"{name}#{attr}","segments":{"name":"person","attr":"attribute"},"examples":["toby#email"]}"#;
        let schema = parse_schema_json(json).unwrap();
        assert_eq!(schema.description, "People");
        assert_eq!(schema.sort_key_format, "{name}#{attr}");
        assert_eq!(schema.segments.len(), 2);
        assert_eq!(schema.examples, vec!["toby#email"]);
    }

    #[test]
    fn test_parse_json_with_fences() {
        let json = "```json\n{\"description\":\"People\",\"sort_key_format\":\"{name}\",\"segments\":{\"name\":\"person\"},\"examples\":[\"toby\"]}\n```";
        let schema = parse_schema_json(json).unwrap();
        assert_eq!(schema.description, "People");
    }

    #[test]
    fn test_parse_invalid_json() {
        assert!(parse_schema_json("not json").is_err());
    }

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

    // --- SchemaStore with tempdb ---

    use crate::backend::MemoryBackend;
    use dynamite_core::api::DynaMite;
    use dynamite_core::types::KeyType;

    fn setup_store() -> (SchemaStore, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = DynaMite::create(dir.path().join("test.db")).unwrap();
        db.create_table("memories")
            .partition_key("category", KeyType::String)
            .sort_key("key", KeyType::String)
            .execute()
            .unwrap();
        let backend = MemoryBackend::Direct(db);
        let store = SchemaStore::new(backend);
        (store, dir)
    }

    #[tokio::test]
    async fn test_store_put_get_schema() {
        let (store, _dir) = setup_store();
        let schema = CategorySchema {
            description: "People and contacts".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: IndexMap::from([
                ("name".into(), "person name".into()),
                ("attribute".into(), "email, phone, role".into()),
            ]),
            examples: vec!["toby#email".into(), "alice#phone".into()],
        };

        store.put_schema("people", &schema).await.unwrap();

        let loaded = store.get_schema("people").await.unwrap().unwrap();
        assert_eq!(loaded.description, "People and contacts");
        assert_eq!(loaded.sort_key_format, "{name}#{attribute}");
        assert_eq!(loaded.segments.len(), 2);
    }

    #[tokio::test]
    async fn test_store_get_nonexistent() {
        let (store, _dir) = setup_store();
        assert!(store.get_schema("nope").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_store_has_schema() {
        let (store, _dir) = setup_store();
        assert!(!store.has_schema("people").await.unwrap());

        let schema = CategorySchema {
            description: "test".into(),
            sort_key_format: "{x}".into(),
            segments: IndexMap::from([("x".into(), "thing".into())]),
            examples: vec![],
        };
        store.put_schema("people", &schema).await.unwrap();
        assert!(store.has_schema("people").await.unwrap());
    }

    #[tokio::test]
    async fn test_store_list_schemas() {
        let (store, _dir) = setup_store();
        let s1 = CategorySchema {
            description: "a".into(),
            sort_key_format: "{x}".into(),
            segments: IndexMap::from([("x".into(), "thing".into())]),
            examples: vec![],
        };
        let s2 = CategorySchema {
            description: "b".into(),
            sort_key_format: "{y}".into(),
            segments: IndexMap::from([("y".into(), "other".into())]),
            examples: vec![],
        };
        store.put_schema("alpha", &s1).await.unwrap();
        store.put_schema("beta", &s2).await.unwrap();

        let list = store.list_schemas().await.unwrap();
        assert_eq!(list.len(), 2);

        let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
    }

    #[tokio::test]
    async fn test_validate_key_no_schema() {
        let (store, _dir) = setup_store();
        // No schema defined = always valid.
        assert!(
            store
                .validate_key("unschematized", "anything")
                .await
                .is_ok()
        );
    }

    #[tokio::test]
    async fn test_validate_key_valid() {
        let (store, _dir) = setup_store();
        let schema = CategorySchema {
            description: "People".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: IndexMap::from([
                ("name".into(), "person name".into()),
                ("attribute".into(), "detail".into()),
            ]),
            examples: vec!["toby#email".into()],
        };
        store.put_schema("people", &schema).await.unwrap();

        assert!(store.validate_key("people", "toby#email").await.is_ok());
        assert!(store.validate_key("people", "alice#phone").await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_key_invalid() {
        let (store, _dir) = setup_store();
        let schema = CategorySchema {
            description: "People".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: IndexMap::from([
                ("name".into(), "person name".into()),
                ("attribute".into(), "detail".into()),
            ]),
            examples: vec!["toby#email".into()],
        };
        store.put_schema("people", &schema).await.unwrap();

        // Missing segment.
        let err = store.validate_key("people", "toby").await.unwrap_err();
        assert!(err.contains("doesn't match expected format"));

        // Empty segment.
        let err = store.validate_key("people", "toby#").await.unwrap_err();
        assert!(err.contains("doesn't match expected format"));
    }

    #[tokio::test]
    async fn test_validate_key_regex_cached() {
        let (store, _dir) = setup_store();
        let schema = CategorySchema {
            description: "test".into(),
            sort_key_format: "{x}".into(),
            segments: IndexMap::from([("x".into(), "thing".into())]),
            examples: vec![],
        };
        store.put_schema("cat", &schema).await.unwrap();

        // First call compiles and caches.
        store.validate_key("cat", "foo").await.unwrap();
        assert!(store.regex_cache.read().await.contains_key("cat"));

        // Second call uses cache.
        store.validate_key("cat", "bar").await.unwrap();
    }

    // --- Inference tests using MockLlmClient ---

    use crate::llm::MockLlmClient;

    #[tokio::test]
    async fn test_infer_schema_success() {
        let mock = MockLlmClient::new(vec![
            r#"{"description":"People","sort_key_format":"{name}#{attribute}","segments":{"name":"person name","attribute":"contact detail"},"examples":["toby#email","alice#phone"]}"#.into(),
        ]);

        let result = infer_schema(&mock, "people", "toby#email", "toby@example.com").await;
        let schema = result.unwrap();
        assert_eq!(schema.description, "People");
        assert_eq!(schema.sort_key_format, "{name}#{attribute}");
        assert_eq!(schema.segments.len(), 2);
    }

    #[tokio::test]
    async fn test_infer_schema_bad_json_returns_none() {
        let mock = MockLlmClient::new(vec!["not valid json".into()]);

        let result = infer_schema(&mock, "people", "toby#email", "toby@example.com").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_infer_schema_with_markdown_fences() {
        let mock = MockLlmClient::new(vec![
            "```json\n{\"description\":\"People\",\"sort_key_format\":\"{name}\",\"segments\":{\"name\":\"person\"},\"examples\":[\"toby\"]}\n```".into(),
        ]);

        let result = infer_schema(&mock, "people", "toby", "toby@example.com").await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_infer_schema_mismatched_segments_returns_none() {
        let mock = MockLlmClient::new(vec![
            r#"{"description":"Bad","sort_key_format":"{a}#{b}","segments":{"a":"only one"},"examples":[]}"#.into(),
        ]);

        let result = infer_schema(&mock, "cat", "x#y", "content").await;
        assert!(result.is_none());
    }

    // --- resolve_query tests ---

    #[tokio::test]
    async fn test_resolve_query_success() {
        let mock = MockLlmClient::new(vec![r#"{"category":"people","prefix":"toby"}"#.into()]);

        let schemas = vec![(
            "people".to_string(),
            CategorySchema {
                description: "Contacts".into(),
                sort_key_format: "{name}#{attribute}".into(),
                segments: IndexMap::from([
                    ("name".into(), "person name".into()),
                    ("attribute".into(), "detail".into()),
                ]),
                examples: vec!["toby#email".into()],
            },
        )];

        let (category, prefix) = resolve_query(&mock, &schemas, "Toby's email")
            .await
            .unwrap();
        assert_eq!(category, "people");
        assert_eq!(prefix.unwrap(), "toby");
    }

    #[tokio::test]
    async fn test_resolve_query_null_prefix() {
        let mock = MockLlmClient::new(vec![r#"{"category":"project","prefix":null}"#.into()]);

        let schemas = vec![(
            "project".to_string(),
            CategorySchema {
                description: "Project info".into(),
                sort_key_format: "{area}#{topic}".into(),
                segments: IndexMap::from([
                    ("area".into(), "area".into()),
                    ("topic".into(), "topic".into()),
                ]),
                examples: vec![],
            },
        )];

        let (category, prefix) = resolve_query(&mock, &schemas, "project details")
            .await
            .unwrap();
        assert_eq!(category, "project");
        assert!(prefix.is_none());
    }

    #[tokio::test]
    async fn test_resolve_query_with_markdown_fences() {
        let mock = MockLlmClient::new(vec![
            "```json\n{\"category\":\"people\",\"prefix\":\"toby\"}\n```".into(),
        ]);

        let schemas = vec![(
            "people".to_string(),
            CategorySchema {
                description: "Contacts".into(),
                sort_key_format: "{name}#{attribute}".into(),
                segments: IndexMap::from([
                    ("name".into(), "person name".into()),
                    ("attribute".into(), "detail".into()),
                ]),
                examples: vec!["toby#email".into()],
            },
        )];

        let (category, prefix) = resolve_query(&mock, &schemas, "Toby's email")
            .await
            .unwrap();
        assert_eq!(category, "people");
        assert_eq!(prefix.unwrap(), "toby");
    }
}
