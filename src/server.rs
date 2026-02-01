use std::sync::Arc;

use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;
use tracing::warn;

use crate::backend::MemoryBackend;
use crate::llm::LlmClient;
use crate::schema::{
    CategorySchema, SCHEMA_CATEGORY, SchemaStore, infer_schema, resolve_query,
    validate_schema_format,
};

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RememberParams {
    #[schemars(
        description = "Semantic category (partition key), e.g. 'rust-patterns', 'project-context'. Cannot be '_schema'."
    )]
    pub category: String,
    #[schemars(
        description = "Entry identifier (sort key), using '#' hierarchy matching the category's schema e.g. 'ownership#borrowing-rules'"
    )]
    pub key: String,
    #[schemars(description = "The memory content to store")]
    pub content: String,
    #[schemars(description = "Optional extra tags or context metadata")]
    pub metadata: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RecallParams {
    #[schemars(description = "Semantic category to query. Required if 'query' is not provided.")]
    pub category: Option<String>,
    #[schemars(description = "Optional sort key prefix for begins_with narrowing")]
    pub prefix: Option<String>,
    #[schemars(
        description = "Natural language query, e.g. \"Toby's email\". The server uses schemas to resolve this to the right category and prefix. Use this OR category+prefix, not both."
    )]
    pub query: Option<String>,
    #[schemars(description = "Maximum number of results (default: 20)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ForgetParams {
    #[schemars(description = "Semantic category of the memory to delete. Cannot be '_schema'.")]
    pub category: String,
    #[schemars(description = "Entry identifier of the memory to delete")]
    pub key: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct DiscoverParams {
    #[schemars(
        description = "If omitted, returns all distinct categories with their schema info. If provided, returns sort key prefixes for that category."
    )]
    pub category: Option<String>,
    #[schemars(description = "Maximum number of results (default: 20)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct DefineParams {
    #[schemars(description = "Category name to define/update a schema for")]
    pub category: String,
    #[schemars(description = "Human-readable description of what this category stores")]
    pub description: String,
    #[schemars(
        description = "Sort key format template, e.g. '{name}#{attribute}'. Segments delimited by '#', enclosed in '{}'."
    )]
    pub sort_key_format: String,
    #[schemars(
        description = "JSON object mapping segment names to descriptions. Must match placeholders in sort_key_format. Example: {\"name\": \"person name\", \"attribute\": \"email, phone, role\"}"
    )]
    pub segments: String,
    #[schemars(description = "Comma-separated example sort keys, e.g. 'toby#email, alice#phone'")]
    pub examples: Option<String>,
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MemoryServer {
    backend: MemoryBackend,
    schema_store: SchemaStore,
    llm: Arc<dyn LlmClient>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl MemoryServer {
    pub fn new(backend: MemoryBackend, schema_store: SchemaStore, llm: Arc<dyn LlmClient>) -> Self {
        Self {
            backend,
            schema_store,
            llm,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Store a memory. Creates or replaces an existing entry. On first write to a new category, the server auto-infers a schema. Subsequent writes are validated against the schema."
    )]
    async fn remember(
        &self,
        Parameters(p): Parameters<RememberParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Reject direct writes to the meta-category.
        if p.category == SCHEMA_CATEGORY {
            return Err(ErrorData::invalid_params(
                "Cannot write directly to the '_schema' category. Use the 'define' tool instead.",
                None,
            ));
        }

        // Check if a schema exists for this category.
        let has_schema = self
            .schema_store
            .has_schema(&p.category)
            .await
            .unwrap_or(false);

        if has_schema {
            // Validate the key against the schema.
            if let Err(msg) = self.schema_store.validate_key(&p.category, &p.key).await {
                return Err(ErrorData::invalid_params(msg, None));
            }
        } else {
            // First write to this category — try to infer a schema.
            if let Some(schema) =
                infer_schema(self.llm.as_ref(), &p.category, &p.key, &p.content).await
                && let Err(e) = self.schema_store.put_schema(&p.category, &schema).await
            {
                warn!(
                    "Failed to store inferred schema for '{}': {}",
                    p.category, e.message
                );
            }
        }

        // Store the memory.
        let mut doc = serde_json::json!({
            "category": p.category,
            "key": p.key,
            "content": p.content,
        });
        if let Some(ref metadata) = p.metadata {
            doc["metadata"] = serde_json::Value::String(metadata.clone());
        }

        self.backend.put_item(doc).await?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Stored memory: {}#{}",
            p.category, p.key
        ))]))
    }

    #[tool(
        description = "Retrieve memories. Provide either a natural language 'query' (resolved via schemas to the right category/prefix) or an explicit 'category' with optional 'prefix'."
    )]
    async fn recall(
        &self,
        Parameters(p): Parameters<RecallParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = p.limit.unwrap_or(20);

        // Determine category and prefix — from query or explicit params.
        let (category, prefix) = if let Some(ref query) = p.query {
            // Natural language resolution.
            let schemas = self.schema_store.list_schemas().await.unwrap_or_default();

            if schemas.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(
                    "No schemas defined yet. Store some memories first, or use 'category' param directly.",
                )]));
            }

            match resolve_query(self.llm.as_ref(), &schemas, query).await {
                Ok((cat, pfx)) => (cat, pfx),
                Err(e) => {
                    return Err(ErrorData::internal_error(
                        format!("Failed to resolve query: {e}"),
                        None,
                    ));
                }
            }
        } else if let Some(ref category) = p.category {
            (category.clone(), p.prefix.clone())
        } else {
            return Err(ErrorData::invalid_params(
                "Either 'query' or 'category' must be provided.",
                None,
            ));
        };

        let items = self
            .backend
            .query(&category, prefix.as_deref(), limit)
            .await?;

        if items.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No memories found.",
            )]));
        }

        let text = serde_json::to_string_pretty(&items).map_err(|e| {
            ErrorData::internal_error(format!("JSON serialization error: {e}"), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "Remove a specific memory by category and key.")]
    async fn forget(
        &self,
        Parameters(p): Parameters<ForgetParams>,
    ) -> Result<CallToolResult, ErrorData> {
        if p.category == SCHEMA_CATEGORY {
            return Err(ErrorData::invalid_params(
                "Cannot delete from '_schema' directly.",
                None,
            ));
        }

        self.backend.delete_item(&p.category, &p.key).await?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Forgot memory: {}#{}",
            p.category, p.key
        ))]))
    }

    #[tool(
        description = "Browse memory structure. Without category: list all categories with their schema descriptions. With category: list sort key prefixes within that category."
    )]
    async fn discover(
        &self,
        Parameters(p): Parameters<DiscoverParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = p.limit.unwrap_or(20);

        if let Some(ref category) = p.category {
            // List sort key prefixes for the given category.
            let items = self.backend.list_sort_key_prefixes(category, limit).await?;

            if items.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(
                    "No entries found.",
                )]));
            }

            // If a schema exists, include the format info.
            let mut text = serde_json::to_string_pretty(&items).map_err(|e| {
                ErrorData::internal_error(format!("JSON serialization error: {e}"), None)
            })?;

            if let Ok(Some(schema)) = self.schema_store.get_schema(category).await {
                text.push_str(&format!(
                    "\n\nSchema: {}\nKey format: {}\nExamples: {:?}",
                    schema.description, schema.sort_key_format, schema.examples
                ));
            }

            Ok(CallToolResult::success(vec![Content::text(text)]))
        } else {
            // List all categories, excluding _schema.
            let items = self.backend.list_partition_keys(limit + 1).await?;

            let filtered: Vec<&serde_json::Value> = items
                .iter()
                .filter(|v| v.as_str() != Some(SCHEMA_CATEGORY))
                .take(limit)
                .collect();

            if filtered.is_empty() {
                return Ok(CallToolResult::success(vec![Content::text(
                    "No entries found.",
                )]));
            }

            // Enrich categories with schema descriptions.
            let schemas = self.schema_store.list_schemas().await.unwrap_or_default();
            let schema_map: std::collections::HashMap<&str, &CategorySchema> = schemas
                .iter()
                .map(|(name, schema)| (name.as_str(), schema))
                .collect();

            let mut lines = Vec::new();
            for cat_val in &filtered {
                if let Some(name) = cat_val.as_str() {
                    if let Some(schema) = schema_map.get(name) {
                        lines.push(format!(
                            "- {name}: {} (key: {})",
                            schema.description, schema.sort_key_format
                        ));
                    } else {
                        lines.push(format!("- {name}"));
                    }
                }
            }

            Ok(CallToolResult::success(vec![Content::text(
                lines.join("\n"),
            )]))
        }
    }

    #[tool(
        description = "Define or update the schema for a memory category. Schemas describe the expected sort key format and its segments."
    )]
    async fn define(
        &self,
        Parameters(p): Parameters<DefineParams>,
    ) -> Result<CallToolResult, ErrorData> {
        // Parse segments JSON.
        let segments: indexmap::IndexMap<String, String> = serde_json::from_str(&p.segments)
            .map_err(|e| {
                ErrorData::invalid_params(
                    format!("'segments' must be a valid JSON object: {e}"),
                    None,
                )
            })?;

        let examples: Vec<String> = p
            .examples
            .as_deref()
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let schema = CategorySchema {
            description: p.description,
            sort_key_format: p.sort_key_format,
            segments,
            examples,
        };

        // Validate the schema is internally consistent.
        validate_schema_format(&schema).map_err(|e| ErrorData::invalid_params(e, None))?;

        self.schema_store.put_schema(&p.category, &schema).await?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Schema defined for category '{}': {}",
            p.category, schema.description
        ))]))
    }
}

#[tool_handler]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Agentic memory server backed by DynaMite. Use 'remember' to store, \
                 'recall' to retrieve (supports natural language queries), 'forget' to delete, \
                 'discover' to browse memory categories and structure, and 'define' to set \
                 category schemas."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::MockLlmClient;
    use dynamite_core::api::DynaMite;
    use dynamite_core::types::KeyType;
    use serde_json::json;

    fn setup_test_db() -> (DynaMite, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = DynaMite::create(dir.path().join("test.db")).unwrap();
        db.create_table("memories")
            .partition_key("category", KeyType::String)
            .sort_key("key", KeyType::String)
            .execute()
            .unwrap();
        (db, dir)
    }

    fn setup_server(db: DynaMite, mock_responses: Vec<String>) -> MemoryServer {
        let backend = MemoryBackend::Direct(db);
        let schema_store = SchemaStore::new(backend.clone());
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(mock_responses));
        MemoryServer::new(backend, schema_store, llm)
    }

    // -----------------------------------------------------------------------
    // Direct DB tests (backward compat with existing tests)
    // -----------------------------------------------------------------------

    #[test]
    fn test_remember_and_recall() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            "memories",
            json!({
                "category": "rust",
                "key": "ownership#borrowing",
                "content": "References allow borrowing without taking ownership"
            }),
        )
        .unwrap();

        let result = db
            .query("memories")
            .partition_key("rust")
            .execute()
            .unwrap();
        assert_eq!(result.items.len(), 1);
        assert_eq!(
            result.items[0]["content"],
            "References allow borrowing without taking ownership"
        );
    }

    #[test]
    fn test_recall_with_prefix() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "ownership#borrowing", "content": "a"}),
        )
        .unwrap();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "ownership#moves", "content": "b"}),
        )
        .unwrap();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "lifetimes#basics", "content": "c"}),
        )
        .unwrap();

        let result = db
            .query("memories")
            .partition_key("rust")
            .sort_key_begins_with("ownership")
            .execute()
            .unwrap();
        assert_eq!(result.items.len(), 2);
    }

    #[test]
    fn test_recall_with_limit() {
        let (db, _dir) = setup_test_db();
        for i in 0..10 {
            db.put_item(
                "memories",
                json!({"category": "bulk", "key": format!("item{i:02}"), "content": format!("c{i}")}),
            )
            .unwrap();
        }

        let result = db
            .query("memories")
            .partition_key("bulk")
            .limit(3)
            .execute()
            .unwrap();
        assert_eq!(result.items.len(), 3);
    }

    #[test]
    fn test_forget() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "temp", "content": "temporary"}),
        )
        .unwrap();

        db.delete_item("memories")
            .partition_key("rust")
            .sort_key("temp")
            .execute()
            .unwrap();

        let item = db
            .get_item("memories")
            .partition_key("rust")
            .sort_key("temp")
            .execute()
            .unwrap();
        assert!(item.is_none());
    }

    #[test]
    fn test_forget_nonexistent_no_error() {
        let (db, _dir) = setup_test_db();
        db.delete_item("memories")
            .partition_key("nonexistent")
            .sort_key("nothing")
            .execute()
            .unwrap();
    }

    #[test]
    fn test_discover_categories() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "a", "content": "x"}),
        )
        .unwrap();
        db.put_item(
            "memories",
            json!({"category": "python", "key": "b", "content": "y"}),
        )
        .unwrap();

        let keys = db.list_partition_keys("memories").execute().unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_discover_prefixes() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "ownership#borrowing", "content": "a"}),
        )
        .unwrap();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "ownership#moves", "content": "b"}),
        )
        .unwrap();
        db.put_item(
            "memories",
            json!({"category": "rust", "key": "lifetimes#basics", "content": "c"}),
        )
        .unwrap();

        let prefixes = db
            .list_sort_key_prefixes("memories")
            .partition_key("rust")
            .execute()
            .unwrap();
        assert_eq!(prefixes.len(), 2);
        assert!(prefixes.contains(&json!("lifetimes")));
        assert!(prefixes.contains(&json!("ownership")));
    }

    #[test]
    fn test_remember_overwrites() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            "memories",
            json!({"category": "test", "key": "item", "content": "old"}),
        )
        .unwrap();
        db.put_item(
            "memories",
            json!({"category": "test", "key": "item", "content": "new"}),
        )
        .unwrap();

        let item = db
            .get_item("memories")
            .partition_key("test")
            .sort_key("item")
            .execute()
            .unwrap()
            .unwrap();
        assert_eq!(item["content"], "new");
    }

    #[test]
    fn test_remember_with_metadata() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            "memories",
            json!({
                "category": "test",
                "key": "with-meta",
                "content": "some content",
                "metadata": "tag:important"
            }),
        )
        .unwrap();

        let item = db
            .get_item("memories")
            .partition_key("test")
            .sort_key("with-meta")
            .execute()
            .unwrap()
            .unwrap();
        assert_eq!(item["metadata"], "tag:important");
    }

    // -----------------------------------------------------------------------
    // MCP tool tests via MemoryServer (with MockLlmClient)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_remember_rejects_schema_category() {
        let (db, _dir) = setup_test_db();
        let server = setup_server(db, vec![]);

        let result = server
            .remember(Parameters(RememberParams {
                category: "_schema".into(),
                key: "test".into(),
                content: "hack".into(),
                metadata: None,
            }))
            .await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("_schema"));
    }

    #[tokio::test]
    async fn test_remember_infers_schema_on_first_write() {
        let (db, _dir) = setup_test_db();
        let server = setup_server(
            db,
            vec![
                // Mock LLM returns a valid schema inference.
                r#"{"description":"People and contacts","sort_key_format":"{name}#{attribute}","segments":{"name":"person name","attribute":"contact detail"},"examples":["toby#email"]}"#.into(),
            ],
        );

        let result = server
            .remember(Parameters(RememberParams {
                category: "people".into(),
                key: "toby#email".into(),
                content: "toby@example.com".into(),
                metadata: None,
            }))
            .await
            .unwrap();

        // Memory stored.
        assert!(
            result.content[0]
                .raw
                .as_text()
                .unwrap()
                .text
                .contains("Stored memory")
        );

        // Schema was inferred and stored.
        let schema = server.schema_store.get_schema("people").await.unwrap();
        assert!(schema.is_some());
        assert_eq!(schema.unwrap().description, "People and contacts");
    }

    #[tokio::test]
    async fn test_remember_validates_key_against_schema() {
        let (db, _dir) = setup_test_db();
        let server = setup_server(db, vec![]);

        // Manually define a schema first.
        let schema = CategorySchema {
            description: "People".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: indexmap::IndexMap::from([
                ("name".into(), "person name".into()),
                ("attribute".into(), "detail".into()),
            ]),
            examples: vec!["toby#email".into()],
        };
        server
            .schema_store
            .put_schema("people", &schema)
            .await
            .unwrap();

        // Valid key.
        let result = server
            .remember(Parameters(RememberParams {
                category: "people".into(),
                key: "toby#email".into(),
                content: "toby@example.com".into(),
                metadata: None,
            }))
            .await;
        assert!(result.is_ok());

        // Invalid key (missing segment).
        let result = server
            .remember(Parameters(RememberParams {
                category: "people".into(),
                key: "toby".into(),
                content: "incomplete".into(),
                metadata: None,
            }))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recall_with_explicit_category() {
        let (db, _dir) = setup_test_db();
        let backend = MemoryBackend::Direct(db);
        let schema_store = SchemaStore::new(backend.clone());
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(vec![]));
        let server = MemoryServer::new(backend.clone(), schema_store, llm);

        // Store via backend directly.
        backend
            .put_item(
                json!({"category": "rust", "key": "ownership#rules", "content": "borrow checker"}),
            )
            .await
            .unwrap();

        let result = server
            .recall(Parameters(RecallParams {
                category: Some("rust".into()),
                prefix: Some("ownership".into()),
                query: None,
                limit: None,
            }))
            .await
            .unwrap();

        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("borrow checker"));
    }

    #[tokio::test]
    async fn test_recall_requires_category_or_query() {
        let (db, _dir) = setup_test_db();
        let server = setup_server(db, vec![]);

        let result = server
            .recall(Parameters(RecallParams {
                category: None,
                prefix: None,
                query: None,
                limit: None,
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_recall_with_query_resolves_via_llm() {
        let (db, _dir) = setup_test_db();
        let backend = MemoryBackend::Direct(db);

        // Define a schema.
        let schema_store = SchemaStore::new(backend.clone());
        let schema = CategorySchema {
            description: "People".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: indexmap::IndexMap::from([
                ("name".into(), "person".into()),
                ("attribute".into(), "detail".into()),
            ]),
            examples: vec!["toby#email".into()],
        };
        schema_store.put_schema("people", &schema).await.unwrap();

        // Store a memory.
        backend
            .put_item(
                json!({"category": "people", "key": "toby#email", "content": "toby@example.com"}),
            )
            .await
            .unwrap();

        // LLM resolves "Toby's email" to category=people, prefix=toby.
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(vec![
            r#"{"category":"people","prefix":"toby"}"#.into(),
        ]));
        let server = MemoryServer::new(backend, schema_store, llm);

        let result = server
            .recall(Parameters(RecallParams {
                category: None,
                prefix: None,
                query: Some("Toby's email".into()),
                limit: None,
            }))
            .await
            .unwrap();

        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("toby@example.com"));
    }

    #[tokio::test]
    async fn test_forget_rejects_schema_category() {
        let (db, _dir) = setup_test_db();
        let server = setup_server(db, vec![]);

        let result = server
            .forget(Parameters(ForgetParams {
                category: "_schema".into(),
                key: "people".into(),
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_discover_hides_schema_category() {
        let (db, _dir) = setup_test_db();
        let backend = MemoryBackend::Direct(db);
        let schema_store = SchemaStore::new(backend.clone());
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(vec![]));

        // Store a schema (creates _schema category).
        let schema = CategorySchema {
            description: "People".into(),
            sort_key_format: "{name}".into(),
            segments: indexmap::IndexMap::from([("name".into(), "person".into())]),
            examples: vec![],
        };
        schema_store.put_schema("people", &schema).await.unwrap();

        // Store a real memory.
        backend
            .put_item(json!({"category": "people", "key": "toby", "content": "data"}))
            .await
            .unwrap();

        let server = MemoryServer::new(backend, schema_store, llm);

        let result = server
            .discover(Parameters(DiscoverParams {
                category: None,
                limit: None,
            }))
            .await
            .unwrap();

        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("people"));
        assert!(!text.contains("_schema"));
    }

    #[tokio::test]
    async fn test_discover_includes_schema_info() {
        let (db, _dir) = setup_test_db();
        let backend = MemoryBackend::Direct(db);
        let schema_store = SchemaStore::new(backend.clone());
        let llm: Arc<dyn LlmClient> = Arc::new(MockLlmClient::new(vec![]));

        let schema = CategorySchema {
            description: "People and contacts".into(),
            sort_key_format: "{name}#{attribute}".into(),
            segments: indexmap::IndexMap::from([
                ("name".into(), "person".into()),
                ("attribute".into(), "detail".into()),
            ]),
            examples: vec![],
        };
        schema_store.put_schema("people", &schema).await.unwrap();

        backend
            .put_item(json!({"category": "people", "key": "toby#email", "content": "data"}))
            .await
            .unwrap();

        let server = MemoryServer::new(backend, schema_store, llm);

        let result = server
            .discover(Parameters(DiscoverParams {
                category: None,
                limit: None,
            }))
            .await
            .unwrap();

        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("People and contacts"));
        assert!(text.contains("{name}#{attribute}"));
    }

    #[tokio::test]
    async fn test_define_creates_schema() {
        let (db, _dir) = setup_test_db();
        let server = setup_server(db, vec![]);

        let result = server
            .define(Parameters(DefineParams {
                category: "people".into(),
                description: "People and contacts".into(),
                sort_key_format: "{name}#{attribute}".into(),
                segments: r#"{"name": "person name", "attribute": "contact detail"}"#.into(),
                examples: Some("toby#email, alice#phone".into()),
            }))
            .await
            .unwrap();

        let text = result.content[0].raw.as_text().unwrap().text.clone();
        assert!(text.contains("Schema defined"));

        // Verify it was stored.
        let schema = server
            .schema_store
            .get_schema("people")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(schema.description, "People and contacts");
        assert_eq!(schema.examples.len(), 2);
    }

    #[tokio::test]
    async fn test_define_rejects_mismatched_segments() {
        let (db, _dir) = setup_test_db();
        let server = setup_server(db, vec![]);

        let result = server
            .define(Parameters(DefineParams {
                category: "bad".into(),
                description: "Bad schema".into(),
                sort_key_format: "{a}#{b}".into(),
                segments: r#"{"a": "only one"}"#.into(),
                examples: None,
            }))
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_backward_compat_no_schema_writes_succeed() {
        let (db, _dir) = setup_test_db();
        // LLM returns invalid response, so schema inference fails silently.
        let server = setup_server(db, vec!["not valid json".into()]);

        let result = server
            .remember(Parameters(RememberParams {
                category: "legacy".into(),
                key: "anything".into(),
                content: "still works".into(),
                metadata: None,
            }))
            .await;

        // Write still succeeds even though inference failed.
        assert!(result.is_ok());
    }
}
