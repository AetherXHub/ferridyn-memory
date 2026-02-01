use rmcp::handler::server::{router::tool::ToolRouter, wrapper::Parameters};
use rmcp::model::{CallToolResult, Content, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData, ServerHandler, tool, tool_handler, tool_router};
use serde::Deserialize;

use crate::backend::MemoryBackend;

// ---------------------------------------------------------------------------
// Parameter structs
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RememberParams {
    #[schemars(
        description = "Semantic category (partition key), e.g. 'rust-patterns', 'project-context'"
    )]
    pub category: String,
    #[schemars(
        description = "Entry identifier (sort key), can use '#' hierarchy e.g. 'ownership#borrowing-rules'"
    )]
    pub key: String,
    #[schemars(description = "The memory content to store")]
    pub content: String,
    #[schemars(description = "Optional extra tags or context metadata")]
    pub metadata: Option<String>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct RecallParams {
    #[schemars(description = "Semantic category to query")]
    pub category: String,
    #[schemars(description = "Optional sort key prefix for begins_with narrowing")]
    pub prefix: Option<String>,
    #[schemars(description = "Maximum number of results (default: 20)")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct ForgetParams {
    #[schemars(description = "Semantic category of the memory to delete")]
    pub category: String,
    #[schemars(description = "Entry identifier of the memory to delete")]
    pub key: String,
}

#[derive(Debug, Deserialize, rmcp::schemars::JsonSchema)]
pub struct DiscoverParams {
    #[schemars(
        description = "If omitted, returns all distinct categories. If provided, returns sort key prefixes for that category."
    )]
    pub category: Option<String>,
    #[schemars(description = "Maximum number of results (default: 20)")]
    pub limit: Option<usize>,
}

// ---------------------------------------------------------------------------
// MCP Server
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct MemoryServer {
    backend: MemoryBackend,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl MemoryServer {
    pub fn new(backend: MemoryBackend) -> Self {
        Self {
            backend,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Store a memory. Creates or replaces an existing entry.")]
    async fn remember(
        &self,
        Parameters(p): Parameters<RememberParams>,
    ) -> Result<CallToolResult, ErrorData> {
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

    #[tool(description = "Retrieve memories by category, optionally filtered by sort key prefix.")]
    async fn recall(
        &self,
        Parameters(p): Parameters<RecallParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = p.limit.unwrap_or(20);

        let items = self
            .backend
            .query(&p.category, p.prefix.as_deref(), limit)
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
        self.backend.delete_item(&p.category, &p.key).await?;

        Ok(CallToolResult::success(vec![Content::text(format!(
            "Forgot memory: {}#{}",
            p.category, p.key
        ))]))
    }

    #[tool(
        description = "Browse memory structure. Without category: list all categories. With category: list sort key prefixes within that category."
    )]
    async fn discover(
        &self,
        Parameters(p): Parameters<DiscoverParams>,
    ) -> Result<CallToolResult, ErrorData> {
        let limit = p.limit.unwrap_or(20);

        let items = if let Some(ref category) = p.category {
            self.backend.list_sort_key_prefixes(category, limit).await?
        } else {
            self.backend.list_partition_keys(limit).await?
        };

        if items.is_empty() {
            return Ok(CallToolResult::success(vec![Content::text(
                "No entries found.",
            )]));
        }

        let text = serde_json::to_string_pretty(&items).map_err(|e| {
            ErrorData::internal_error(format!("JSON serialization error: {e}"), None)
        })?;

        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

#[tool_handler]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Agentic memory server backed by DynaMite. Use 'remember' to store, \
                 'recall' to retrieve, 'forget' to delete, and 'discover' to browse \
                 memory categories and structure."
                    .to_string(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
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
}
