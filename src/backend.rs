//! Backend abstraction: server client (production) or direct FerridynDB handle (tests only).

use std::sync::Arc;

use crate::error::MemoryError;
use crate::schema::{PREDEFINED_SCHEMAS, SchemaManager};
use serde_json::Value;
use tokio::sync::Mutex;

#[cfg(test)]
use ferridyn_core::api::FerridynDB;
use ferridyn_server::FerridynClient;
use ferridyn_server::client::{AttributeDefInput, IndexInfo, PartitionSchemaInfo};

/// Inner storage variant for [`MemoryBackend`].
#[derive(Clone)]
enum BackendInner {
    #[cfg(test)]
    Direct(FerridynDB),
    Server(Arc<Mutex<FerridynClient>>),
}

/// Unified backend for memory operations.
///
/// Wraps either a server client (production) or direct FerridynDB handle (tests)
/// along with the table name to operate on. The table name is determined by
/// the namespace configuration.
#[derive(Clone)]
pub struct MemoryBackend {
    inner: BackendInner,
    /// The table name used for all operations (e.g. "memories" or "memories_myproject").
    pub table_name: String,
}

impl MemoryBackend {
    /// Create a backend connected to a ferridyn-server.
    pub fn server(client: Arc<Mutex<FerridynClient>>, table_name: String) -> Self {
        Self {
            inner: BackendInner::Server(client),
            table_name,
        }
    }

    /// Create a backend with a direct in-process database (tests only).
    #[cfg(test)]
    pub fn direct(db: FerridynDB, table_name: String) -> Self {
        Self {
            inner: BackendInner::Direct(db),
            table_name,
        }
    }

    pub async fn put_item(&self, doc: Value) -> Result<(), MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(db) => db.put_item(&self.table_name, doc).map_err(mcp_core_err),
            BackendInner::Server(client) => client
                .lock()
                .await
                .put_item(&self.table_name, doc)
                .await
                .map_err(mcp_client_err),
        }
    }

    pub async fn get_item(&self, category: &str, key: &str) -> Result<Option<Value>, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(db) => db
                .get_item(&self.table_name)
                .partition_key(category)
                .sort_key(key)
                .execute()
                .map_err(mcp_core_err),
            BackendInner::Server(client) => client
                .lock()
                .await
                .get_item(
                    &self.table_name,
                    Value::String(category.to_string()),
                    Some(Value::String(key.to_string())),
                )
                .await
                .map_err(mcp_client_err),
        }
    }

    pub async fn query(
        &self,
        partition_key: &str,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(db) => {
                let mut builder = db.query(&self.table_name).partition_key(partition_key);
                if let Some(pfx) = prefix {
                    builder = builder.sort_key_begins_with(pfx);
                }
                let result = builder.limit(limit).execute().map_err(mcp_core_err)?;
                Ok(result.items)
            }
            BackendInner::Server(client) => {
                use ferridyn_server::protocol::SortKeyCondition;
                let cond = prefix.map(|pfx| SortKeyCondition::BeginsWith {
                    prefix: pfx.to_string(),
                });
                let result = client
                    .lock()
                    .await
                    .query(
                        &self.table_name,
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

    pub async fn delete_item(&self, category: &str, key: &str) -> Result<(), MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(db) => db
                .delete_item(&self.table_name)
                .partition_key(category)
                .sort_key(key)
                .execute()
                .map_err(mcp_core_err),
            BackendInner::Server(client) => client
                .lock()
                .await
                .delete_item(
                    &self.table_name,
                    Value::String(category.to_string()),
                    Some(Value::String(key.to_string())),
                )
                .await
                .map_err(mcp_client_err),
        }
    }

    pub async fn list_partition_keys(&self, limit: usize) -> Result<Vec<Value>, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(db) => db
                .list_partition_keys(&self.table_name)
                .limit(limit)
                .execute()
                .map_err(mcp_core_err),
            BackendInner::Server(client) => client
                .lock()
                .await
                .list_partition_keys(&self.table_name, Some(limit))
                .await
                .map_err(mcp_client_err),
        }
    }

    pub async fn list_sort_key_prefixes(
        &self,
        category: &str,
        limit: usize,
    ) -> Result<Vec<Value>, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(db) => db
                .list_sort_key_prefixes(&self.table_name)
                .partition_key(category)
                .limit(limit)
                .execute()
                .map_err(mcp_core_err),
            BackendInner::Server(client) => client
                .lock()
                .await
                .list_sort_key_prefixes(
                    &self.table_name,
                    Value::String(category.to_string()),
                    Some(limit),
                )
                .await
                .map_err(mcp_client_err),
        }
    }

    // -- Partition schema operations --

    pub async fn create_schema(
        &self,
        prefix: &str,
        description: Option<&str>,
        attrs: &[AttributeDefInput],
        validate: bool,
    ) -> Result<(), MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "schema operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .create_schema(&self.table_name, prefix, description, attrs, validate)
                .await
                .map_err(|e| MemoryError::Schema(e.to_string())),
        }
    }

    pub async fn describe_schema(&self, prefix: &str) -> Result<PartitionSchemaInfo, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "schema operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .describe_schema(&self.table_name, prefix)
                .await
                .map_err(|e| MemoryError::Schema(e.to_string())),
        }
    }

    pub async fn list_schemas(&self) -> Result<Vec<PartitionSchemaInfo>, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "schema operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .list_schemas(&self.table_name)
                .await
                .map_err(|e| MemoryError::Schema(e.to_string())),
        }
    }

    pub async fn drop_schema(&self, prefix: &str) -> Result<(), MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "schema operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .drop_schema(&self.table_name, prefix)
                .await
                .map_err(|e| MemoryError::Schema(e.to_string())),
        }
    }

    // -- Secondary index operations --

    pub async fn create_index(
        &self,
        name: &str,
        partition_schema: &str,
        key_name: &str,
        key_type: &str,
    ) -> Result<(), MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "index operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .create_index(&self.table_name, name, partition_schema, key_name, key_type)
                .await
                .map_err(|e| MemoryError::Index(e.to_string())),
        }
    }

    pub async fn list_indexes(&self) -> Result<Vec<IndexInfo>, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "index operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .list_indexes(&self.table_name)
                .await
                .map_err(|e| MemoryError::Index(e.to_string())),
        }
    }

    pub async fn describe_index(&self, name: &str) -> Result<IndexInfo, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "index operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .describe_index(&self.table_name, name)
                .await
                .map_err(|e| MemoryError::Index(e.to_string())),
        }
    }

    pub async fn drop_index(&self, name: &str) -> Result<(), MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "index operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => client
                .lock()
                .await
                .drop_index(&self.table_name, name)
                .await
                .map_err(|e| MemoryError::Index(e.to_string())),
        }
    }

    /// Create all predefined schemas and their indexes if they don't already exist.
    ///
    /// Idempotent — skips categories that already have schemas.
    /// Called by `fmemory init` and auto-init on first `remember`.
    pub async fn ensure_predefined_schemas(&self) -> Result<(), MemoryError> {
        let sm = SchemaManager::new(self.clone());
        for predefined in PREDEFINED_SCHEMAS {
            if sm.has_schema(predefined.name).await? {
                continue;
            }
            let definition = predefined.to_definition();
            sm.create_schema_with_indexes(predefined.name, &definition, false)
                .await?;
        }
        Ok(())
    }

    pub async fn query_index(
        &self,
        index_name: &str,
        key_value: Value,
        limit: Option<usize>,
    ) -> Result<Vec<Value>, MemoryError> {
        match &self.inner {
            #[cfg(test)]
            BackendInner::Direct(_) => Err(MemoryError::Internal(
                "index operations not supported in direct mode".into(),
            )),
            BackendInner::Server(client) => {
                let result = client
                    .lock()
                    .await
                    .query_index(&self.table_name, index_name, key_value, limit, None)
                    .await
                    .map_err(|e| MemoryError::Index(e.to_string()))?;
                Ok(result.items)
            }
        }
    }
}

#[cfg(test)]
fn mcp_core_err(err: ferridyn_core::error::Error) -> MemoryError {
    MemoryError::Internal(format!("{err}"))
}

fn mcp_client_err(err: ferridyn_server::error::ClientError) -> MemoryError {
    MemoryError::Server(format!("{err}"))
}

#[cfg(test)]
mod tests {
    use crate::TABLE_NAME;
    use ferridyn_core::api::FerridynDB;
    use ferridyn_core::types::KeyType;
    use serde_json::json;

    fn setup_test_db() -> (FerridynDB, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = FerridynDB::create(dir.path().join("test.db")).unwrap();
        db.create_table(TABLE_NAME)
            .partition_key("category", KeyType::String)
            .sort_key("key", KeyType::String)
            .execute()
            .unwrap();
        (db, dir)
    }

    #[test]
    fn test_remember_and_recall() {
        let (db, _dir) = setup_test_db();
        db.put_item(TABLE_NAME, json!({"category": "rust", "key": "ownership#borrowing", "content": "References allow borrowing without taking ownership"})).unwrap();
        let result = db
            .query(TABLE_NAME)
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
            TABLE_NAME,
            json!({"category": "rust", "key": "ownership#borrowing", "content": "a"}),
        )
        .unwrap();
        db.put_item(
            TABLE_NAME,
            json!({"category": "rust", "key": "ownership#moves", "content": "b"}),
        )
        .unwrap();
        db.put_item(
            TABLE_NAME,
            json!({"category": "rust", "key": "lifetimes#basics", "content": "c"}),
        )
        .unwrap();
        let result = db
            .query(TABLE_NAME)
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
            db.put_item(TABLE_NAME, json!({"category": "bulk", "key": format!("item{i:02}"), "content": format!("c{i}")})).unwrap();
        }
        let result = db
            .query(TABLE_NAME)
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
            TABLE_NAME,
            json!({"category": "rust", "key": "temp", "content": "temporary"}),
        )
        .unwrap();
        db.delete_item(TABLE_NAME)
            .partition_key("rust")
            .sort_key("temp")
            .execute()
            .unwrap();
        let item = db
            .get_item(TABLE_NAME)
            .partition_key("rust")
            .sort_key("temp")
            .execute()
            .unwrap();
        assert!(item.is_none());
    }

    #[test]
    fn test_forget_nonexistent_no_error() {
        let (db, _dir) = setup_test_db();
        db.delete_item(TABLE_NAME)
            .partition_key("nonexistent")
            .sort_key("nothing")
            .execute()
            .unwrap();
    }

    #[test]
    fn test_discover_categories() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            TABLE_NAME,
            json!({"category": "rust", "key": "a", "content": "x"}),
        )
        .unwrap();
        db.put_item(
            TABLE_NAME,
            json!({"category": "python", "key": "b", "content": "y"}),
        )
        .unwrap();
        let keys = db.list_partition_keys(TABLE_NAME).execute().unwrap();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_discover_prefixes() {
        let (db, _dir) = setup_test_db();
        db.put_item(
            TABLE_NAME,
            json!({"category": "rust", "key": "ownership#borrowing", "content": "a"}),
        )
        .unwrap();
        db.put_item(
            TABLE_NAME,
            json!({"category": "rust", "key": "ownership#moves", "content": "b"}),
        )
        .unwrap();
        db.put_item(
            TABLE_NAME,
            json!({"category": "rust", "key": "lifetimes#basics", "content": "c"}),
        )
        .unwrap();
        let prefixes = db
            .list_sort_key_prefixes(TABLE_NAME)
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
            TABLE_NAME,
            json!({"category": "test", "key": "item", "content": "old"}),
        )
        .unwrap();
        db.put_item(
            TABLE_NAME,
            json!({"category": "test", "key": "item", "content": "new"}),
        )
        .unwrap();
        let item = db
            .get_item(TABLE_NAME)
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
        db.put_item(TABLE_NAME, json!({"category": "test", "key": "with-meta", "content": "some content", "metadata": "tag:important"})).unwrap();
        let item = db
            .get_item(TABLE_NAME)
            .partition_key("test")
            .sort_key("with-meta")
            .execute()
            .unwrap()
            .unwrap();
        assert_eq!(item["metadata"], "tag:important");
    }

    #[test]
    fn test_backend_put_and_query() {
        use super::MemoryBackend;
        let (db, _dir) = setup_test_db();
        let backend = MemoryBackend::direct(db, TABLE_NAME.to_string());
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            backend
                .put_item(json!({"category": "test", "key": "a", "content": "hello"}))
                .await
                .unwrap();
            let items = backend.query("test", None, 10).await.unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0]["content"], "hello");
        });
    }

    #[test]
    fn test_resolve_table_name() {
        use crate::resolve_table_name;
        assert_eq!(resolve_table_name(None), "memories");
        assert_eq!(resolve_table_name(Some("myproject")), "memories_myproject");
        assert_eq!(resolve_table_name(Some("test")), "memories_test");
    }

    #[test]
    fn test_backend_uses_custom_table_name() {
        use super::MemoryBackend;
        let dir = tempfile::tempdir().unwrap();
        let db = FerridynDB::create(dir.path().join("test.db")).unwrap();
        db.create_table("memories_myns")
            .partition_key("category", KeyType::String)
            .sort_key("key", KeyType::String)
            .execute()
            .unwrap();
        let backend = MemoryBackend::direct(db, "memories_myns".to_string());
        assert_eq!(backend.table_name, "memories_myns");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            backend
                .put_item(json!({"category": "test", "key": "a", "content": "namespaced"}))
                .await
                .unwrap();
            let items = backend.query("test", None, 10).await.unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0]["content"], "namespaced");
        });
    }
}
