//! MCP (Model Context Protocol) server interface for memory operations.
//!
//! Exposes memory operations as MCP tools for AI agents via stdio transport.
//! No LLM calls — agents provide structured data directly.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router,
    transport::stdio,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;

use crate::backend::MemoryBackend;
use crate::resolve_table_name;
use crate::schema::{PREDEFINED_SCHEMAS, SchemaManager};
use crate::ttl::{
    INTERACTIONS_DEFAULT_TTL, SCRATCHPAD_DEFAULT_TTL, SESSIONS_DEFAULT_TTL, compute_expires_at,
    filter_expired, is_expired, parse_ttl,
};

// ============================================================================
// Tool Input Schemas
// ============================================================================

/// Parameters for storing a memory item.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct StoreParams {
    /// Memory category (e.g. "project", "decisions", "contacts").
    pub category: String,
    /// Unique key within the category.
    pub key: String,
    /// Structured attributes as a JSON object.
    pub attributes: serde_json::Map<String, Value>,
    /// Optional TTL (e.g. "24h", "7d", "2w").
    #[schemars(description = "Time-to-live: 24h, 7d, 30d, etc.")]
    pub ttl: Option<String>,
    /// Optional namespace override for this operation.
    pub namespace: Option<String>,
}

/// Parameters for retrieving a specific memory.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct GetParams {
    /// Memory category.
    pub category: String,
    /// Item key.
    pub key: String,
    /// Optional namespace override.
    pub namespace: Option<String>,
}

/// Parameters for querying memories in a category.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct QueryParams {
    /// Memory category to query.
    pub category: String,
    /// Optional key prefix for begins_with matching.
    pub prefix: Option<String>,
    /// Maximum number of results (default: 20).
    pub limit: Option<usize>,
    /// Optional namespace override.
    pub namespace: Option<String>,
}

/// Parameters for deleting a specific memory.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct DeleteParams {
    /// Memory category.
    pub category: String,
    /// Item key.
    pub key: String,
    /// Optional namespace override.
    pub namespace: Option<String>,
}

/// Parameters for listing categories or keys.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListParams {
    /// If provided, list keys within this category. Otherwise list all categories.
    pub category: Option<String>,
    /// Optional namespace override.
    pub namespace: Option<String>,
}

/// Parameters for showing schema definitions.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct SchemaParams {
    /// If provided, show schema for this category. Otherwise list all schemas.
    pub category: Option<String>,
    /// Optional namespace override.
    pub namespace: Option<String>,
}

/// Parameters for promoting a memory (remove TTL, optionally re-categorize).
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PromoteParams {
    /// Source category.
    pub category: String,
    /// Item key.
    pub key: String,
    /// Optional target category for re-categorization.
    pub to_category: Option<String>,
    /// Optional namespace override.
    pub namespace: Option<String>,
}

/// Parameters for pruning expired memories.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct PruneParams {
    /// If provided, only prune this category.
    pub category: Option<String>,
    /// Optional namespace override.
    pub namespace: Option<String>,
}

/// Parameters for initializing predefined schemas.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
pub struct InitParams {
    /// Optional namespace override.
    pub namespace: Option<String>,
    /// Recreate schemas even if they already exist.
    pub force: Option<bool>,
}

// ============================================================================
// MCP Server
// ============================================================================

/// MCP server exposing memory operations as tools.
#[derive(Clone)]
pub struct MemoryServer {
    backend: Arc<Mutex<MemoryBackend>>,
    default_namespace: Option<String>,
    tool_router: ToolRouter<Self>,
}

impl MemoryServer {
    /// Create a new MCP memory server.
    pub fn new(backend: MemoryBackend, default_namespace: Option<String>) -> Self {
        Self {
            backend: Arc::new(Mutex::new(backend)),
            default_namespace,
            tool_router: Self::tool_router(),
        }
    }

    /// Resolve a backend for the given namespace override, or use the default.
    async fn resolve_backend(&self, namespace: &Option<String>) -> MemoryBackend {
        let mut backend = self.backend.lock().await.clone();
        if let Some(ns) = namespace.as_ref().or(self.default_namespace.as_ref()) {
            backend.table_name = resolve_table_name(Some(ns));
        }
        backend
    }
}

fn err(msg: impl Into<String>) -> McpError {
    McpError::internal_error(msg.into(), None)
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for MemoryServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "fmemory".into(),
                title: Some("FerridynDB Memory".into()),
                version: env!("CARGO_PKG_VERSION").into(),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Persistent structured memory storage. \
                 Store, query, and manage memories organized by category."
                    .into(),
            ),
        }
    }
}

#[tool_router(router = tool_router)]
impl MemoryServer {
    /// Store a structured memory item.
    #[tool(
        name = "memory_store",
        description = "Store a structured memory item with category, key, and typed attributes"
    )]
    async fn memory_store(
        &self,
        Parameters(params): Parameters<StoreParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;

        let mut doc = serde_json::json!({
            "category": params.category,
            "key": params.key,
        });

        // Merge attributes into the document.
        for (k, v) in &params.attributes {
            doc[k] = v.clone();
        }

        // Auto-inject created_at.
        doc["created_at"] = Value::String(chrono::Utc::now().to_rfc3339());

        // Handle TTL: explicit > category default.
        if let Some(ref ttl_str) = params.ttl {
            let duration = parse_ttl(ttl_str).map_err(err)?;
            doc["expires_at"] = Value::String(compute_expires_at(duration));
        } else if params.category == "scratchpad" {
            doc["expires_at"] = Value::String(compute_expires_at(SCRATCHPAD_DEFAULT_TTL));
        } else if params.category == "sessions" {
            doc["expires_at"] = Value::String(compute_expires_at(SESSIONS_DEFAULT_TTL));
        } else if params.category == "interactions" {
            doc["expires_at"] = Value::String(compute_expires_at(INTERACTIONS_DEFAULT_TTL));
        }

        backend
            .put_item(doc.clone())
            .await
            .map_err(|e| err(e.to_string()))?;

        let result = serde_json::json!({
            "stored": format!("{}/{}", params.category, params.key),
        });
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result).unwrap(),
        )]))
    }

    /// Retrieve a specific memory by category and key.
    #[tool(
        name = "memory_get",
        description = "Retrieve a specific memory by category and key"
    )]
    async fn memory_get(
        &self,
        Parameters(params): Parameters<GetParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;

        let item = backend
            .get_item(&params.category, &params.key)
            .await
            .map_err(|e| err(e.to_string()))?;

        match item {
            Some(item) if !is_expired(&item) => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&item).unwrap(),
            )])),
            _ => Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string(&serde_json::json!({"error": "not_found"})).unwrap(),
            )])),
        }
    }

    /// Query memories in a category with optional prefix filtering.
    #[tool(
        name = "memory_query",
        description = "Query memories in a category, optionally filtering by key prefix"
    )]
    async fn memory_query(
        &self,
        Parameters(params): Parameters<QueryParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;
        let limit = params.limit.unwrap_or(20);

        let items = backend
            .query(&params.category, params.prefix.as_deref(), limit)
            .await
            .map_err(|e| err(e.to_string()))?;

        let items = filter_expired(items);

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&items).unwrap(),
        )]))
    }

    /// Delete a specific memory.
    #[tool(
        name = "memory_delete",
        description = "Delete a specific memory by category and key"
    )]
    async fn memory_delete(
        &self,
        Parameters(params): Parameters<DeleteParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;

        backend
            .delete_item(&params.category, &params.key)
            .await
            .map_err(|e| err(e.to_string()))?;

        let result = serde_json::json!({
            "deleted": format!("{}/{}", params.category, params.key),
        });
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result).unwrap(),
        )]))
    }

    /// List categories or keys within a category.
    #[tool(
        name = "memory_list",
        description = "List all categories, or list keys within a specific category"
    )]
    async fn memory_list(
        &self,
        Parameters(params): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;

        if let Some(ref cat) = params.category {
            let items = backend
                .query(cat, None, 100)
                .await
                .map_err(|e| err(e.to_string()))?;
            let items = filter_expired(items);
            let keys: Vec<&str> = items
                .iter()
                .filter_map(|item| item["key"].as_str())
                .collect();
            let result = serde_json::json!({
                "category": cat,
                "keys": keys,
            });
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap(),
            )]))
        } else {
            let keys = backend
                .list_partition_keys(100)
                .await
                .map_err(|e| err(e.to_string()))?;
            let categories: Vec<&str> = keys.iter().filter_map(|v| v.as_str()).collect();
            let result = serde_json::json!({ "categories": categories });
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap(),
            )]))
        }
    }

    /// Show schema definitions for categories.
    #[tool(
        name = "memory_schema",
        description = "Show schema definitions for a category or list all schemas"
    )]
    async fn memory_schema(
        &self,
        Parameters(params): Parameters<SchemaParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;
        let sm = SchemaManager::new(backend);

        if let Some(ref cat) = params.category {
            let schema = sm.get_schema(cat).await.map_err(|e| err(e.to_string()))?;
            match schema {
                Some(s) => {
                    let result = serde_json::json!({
                        "category": cat,
                        "description": s.description,
                        "attributes": s.attributes.iter().map(|a| serde_json::json!({
                            "name": a.name,
                            "type": a.attr_type,
                            "required": a.required,
                        })).collect::<Vec<_>>(),
                    });
                    Ok(CallToolResult::success(vec![Content::text(
                        serde_json::to_string_pretty(&result).unwrap(),
                    )]))
                }
                None => Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string(&serde_json::json!({"error": "schema_not_found"}))
                        .unwrap(),
                )])),
            }
        } else {
            let schemas = sm.list_schemas().await.map_err(|e| err(e.to_string()))?;
            let result: Vec<Value> = schemas
                .iter()
                .map(|s| {
                    serde_json::json!({
                        "category": s.prefix,
                        "description": s.description,
                        "attribute_count": s.attributes.len(),
                    })
                })
                .collect();
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string_pretty(&result).unwrap(),
            )]))
        }
    }

    /// Promote a memory: remove TTL, optionally re-categorize.
    #[tool(
        name = "memory_promote",
        description = "Promote a memory to long-term (remove TTL), optionally move to a new category"
    )]
    async fn memory_promote(
        &self,
        Parameters(params): Parameters<PromoteParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;

        let item = backend
            .get_item(&params.category, &params.key)
            .await
            .map_err(|e| err(e.to_string()))?;

        let item = match item {
            Some(i) => i,
            None => {
                return Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string(&serde_json::json!({"error": "not_found"})).unwrap(),
                )]));
            }
        };

        let target_category = params.to_category.as_deref().unwrap_or(&params.category);

        if target_category != params.category {
            // Move to new category: copy item as-is (no LLM re-parsing).
            let mut promoted = serde_json::json!({
                "category": target_category,
                "key": params.key,
            });
            if let Some(obj) = item.as_object() {
                for (k, v) in obj {
                    if k == "key" || k == "category" || k == "expires_at" {
                        continue;
                    }
                    promoted[k] = v.clone();
                }
            }
            promoted["created_at"] = Value::String(chrono::Utc::now().to_rfc3339());

            backend
                .put_item(promoted)
                .await
                .map_err(|e| err(e.to_string()))?;
            backend
                .delete_item(&params.category, &params.key)
                .await
                .map_err(|e| err(e.to_string()))?;

            let result = serde_json::json!({
                "promoted": true,
                "from": format!("{}/{}", params.category, params.key),
                "to": format!("{}/{}", target_category, params.key),
            });
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string(&result).unwrap(),
            )]))
        } else {
            // Same category: just remove expires_at.
            let mut promoted = item.clone();
            if let Some(obj) = promoted.as_object_mut() {
                obj.remove("expires_at");
            }
            promoted["created_at"] = Value::String(chrono::Utc::now().to_rfc3339());

            backend
                .put_item(promoted)
                .await
                .map_err(|e| err(e.to_string()))?;

            let result = serde_json::json!({
                "promoted": true,
                "category": params.category,
                "key": params.key,
            });
            Ok(CallToolResult::success(vec![Content::text(
                serde_json::to_string(&result).unwrap(),
            )]))
        }
    }

    /// Delete all expired memories.
    #[tool(
        name = "memory_prune",
        description = "Delete all expired memories, optionally within a specific category"
    )]
    async fn memory_prune(
        &self,
        Parameters(params): Parameters<PruneParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;
        let sm = SchemaManager::new(backend.clone());

        let categories: Vec<String> = if let Some(ref cat) = params.category {
            vec![cat.clone()]
        } else {
            let schemas = sm.list_schemas().await.map_err(|e| err(e.to_string()))?;
            schemas.iter().map(|s| s.prefix.clone()).collect()
        };

        let mut total_pruned = 0usize;
        for cat in &categories {
            let items = backend
                .query(cat, None, 1000)
                .await
                .map_err(|e| err(e.to_string()))?;
            for item in &items {
                if is_expired(item)
                    && let Some(key) = item["key"].as_str()
                {
                    backend
                        .delete_item(cat, key)
                        .await
                        .map_err(|e| err(e.to_string()))?;
                    total_pruned += 1;
                }
            }
        }

        let result = serde_json::json!({ "pruned": total_pruned });
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result).unwrap(),
        )]))
    }

    /// Initialize predefined schemas and indexes.
    #[tool(
        name = "memory_init",
        description = "Initialize predefined category schemas and indexes"
    )]
    async fn memory_init(
        &self,
        Parameters(params): Parameters<InitParams>,
    ) -> Result<CallToolResult, McpError> {
        let backend = self.resolve_backend(&params.namespace).await;

        if params.force.unwrap_or(false) {
            let sm = SchemaManager::new(backend.clone());
            for predefined in PREDEFINED_SCHEMAS {
                let _ = backend.drop_schema(predefined.name).await;
                let indexes = sm.list_indexes().await.unwrap_or_default();
                for idx in &indexes {
                    if idx.partition_schema == predefined.name {
                        let _ = backend.drop_index(&idx.name).await;
                    }
                }
            }
        }

        backend
            .ensure_predefined_schemas()
            .await
            .map_err(|e| err(e.to_string()))?;

        let names: Vec<&str> = PREDEFINED_SCHEMAS.iter().map(|s| s.name).collect();
        let result = serde_json::json!({ "initialized": names });
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string(&result).unwrap(),
        )]))
    }
}

// ============================================================================
// Entry Point
// ============================================================================

/// Run the MCP server on stdio transport.
pub async fn run_mcp_server(
    backend: MemoryBackend,
    namespace: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let server = MemoryServer::new(backend, namespace);
    let service = server.serve(stdio()).await.map_err(|e| e.to_string())?;
    service.waiting().await.map_err(|e| e.to_string())?;
    Ok(())
}
