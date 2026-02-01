//! Backend abstraction: direct DynaMite handle or server client.

use std::sync::Arc;

use rmcp::ErrorData;
use serde_json::Value;
use tokio::sync::Mutex;

use dynamite_core::api::DynaMite;
use dynamite_server::DynaMiteClient;

use crate::TABLE_NAME;

/// Unified backend for memory operations.
///
/// `Direct` uses an in-process DynaMite handle (exclusive file lock).
/// `Server` uses a DynaMiteClient connected to a running `dynamite-server`.
#[derive(Clone)]
pub enum MemoryBackend {
    Direct(DynaMite),
    Server(Arc<Mutex<DynaMiteClient>>),
}

impl MemoryBackend {
    pub async fn put_item(&self, doc: Value) -> Result<(), ErrorData> {
        match self {
            Self::Direct(db) => db.put_item(TABLE_NAME, doc).map_err(mcp_core_err),
            Self::Server(client) => client
                .lock()
                .await
                .put_item(TABLE_NAME, doc)
                .await
                .map_err(mcp_client_err),
        }
    }

    pub async fn query(
        &self,
        partition_key: &str,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, ErrorData> {
        match self {
            Self::Direct(db) => {
                let mut builder = db.query(TABLE_NAME).partition_key(partition_key);
                if let Some(pfx) = prefix {
                    builder = builder.sort_key_begins_with(pfx);
                }
                let result = builder.limit(limit).execute().map_err(mcp_core_err)?;
                Ok(result.items)
            }
            Self::Server(client) => {
                use dynamite_server::protocol::SortKeyCondition;
                let cond = prefix.map(|pfx| SortKeyCondition::BeginsWith {
                    prefix: pfx.to_string(),
                });
                let result = client
                    .lock()
                    .await
                    .query(
                        TABLE_NAME,
                        Value::String(partition_key.to_string()),
                        cond,
                        Some(limit),
                        None,
                        None,
                    )
                    .await
                    .map_err(mcp_client_err)?;
                Ok(result.items)
            }
        }
    }

    pub async fn delete_item(&self, category: &str, key: &str) -> Result<(), ErrorData> {
        match self {
            Self::Direct(db) => db
                .delete_item(TABLE_NAME)
                .partition_key(category)
                .sort_key(key)
                .execute()
                .map_err(mcp_core_err),
            Self::Server(client) => client
                .lock()
                .await
                .delete_item(
                    TABLE_NAME,
                    Value::String(category.to_string()),
                    Some(Value::String(key.to_string())),
                )
                .await
                .map_err(mcp_client_err),
        }
    }

    pub async fn list_partition_keys(&self, limit: usize) -> Result<Vec<Value>, ErrorData> {
        match self {
            Self::Direct(db) => db
                .list_partition_keys(TABLE_NAME)
                .limit(limit)
                .execute()
                .map_err(mcp_core_err),
            Self::Server(client) => client
                .lock()
                .await
                .list_partition_keys(TABLE_NAME, Some(limit))
                .await
                .map_err(mcp_client_err),
        }
    }

    pub async fn list_sort_key_prefixes(
        &self,
        category: &str,
        limit: usize,
    ) -> Result<Vec<Value>, ErrorData> {
        match self {
            Self::Direct(db) => db
                .list_sort_key_prefixes(TABLE_NAME)
                .partition_key(category)
                .limit(limit)
                .execute()
                .map_err(mcp_core_err),
            Self::Server(client) => client
                .lock()
                .await
                .list_sort_key_prefixes(
                    TABLE_NAME,
                    Value::String(category.to_string()),
                    Some(limit),
                )
                .await
                .map_err(mcp_client_err),
        }
    }
}

fn mcp_core_err(err: dynamite_core::error::Error) -> ErrorData {
    ErrorData::internal_error(format!("Database error: {err}"), None)
}

fn mcp_client_err(err: dynamite_server::error::ClientError) -> ErrorData {
    ErrorData::internal_error(format!("Server error: {err}"), None)
}
