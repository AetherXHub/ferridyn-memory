//! Backend abstraction: direct FerridynDB handle or server client.

use std::sync::Arc;

use crate::error::MemoryError;
use serde_json::Value;
use tokio::sync::Mutex;

use ferridyn_core::api::FerridynDB;
use ferridyn_server::FerridynClient;

use crate::TABLE_NAME;

/// Unified backend for memory operations.
///
/// `Direct` uses an in-process FerridynDB handle (exclusive file lock).
/// `Server` uses a FerridynClient connected to a running `ferridyn-server`.
#[derive(Clone)]
pub enum MemoryBackend {
    Direct(FerridynDB),
    Server(Arc<Mutex<FerridynClient>>),
}

impl MemoryBackend {
    pub async fn put_item(&self, doc: Value) -> Result<(), MemoryError> {
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

    pub async fn get_item(&self, category: &str, key: &str) -> Result<Option<Value>, MemoryError> {
        match self {
            Self::Direct(db) => db
                .get_item(TABLE_NAME)
                .partition_key(category)
                .sort_key(key)
                .execute()
                .map_err(mcp_core_err),
            Self::Server(client) => client
                .lock()
                .await
                .get_item(
                    TABLE_NAME,
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
                use ferridyn_server::protocol::SortKeyCondition;
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

    pub async fn delete_item(&self, category: &str, key: &str) -> Result<(), MemoryError> {
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

    pub async fn list_partition_keys(&self, limit: usize) -> Result<Vec<Value>, MemoryError> {
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
    ) -> Result<Vec<Value>, MemoryError> {
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

fn mcp_core_err(err: ferridyn_core::error::Error) -> MemoryError {
    MemoryError::Database(format!("{err}"))
}

fn mcp_client_err(err: ferridyn_server::error::ClientError) -> MemoryError {
    MemoryError::Server(format!("{err}"))
}

#[cfg(test)]
mod tests {
    use ferridyn_core::api::FerridynDB;
    use ferridyn_core::types::KeyType;
    use serde_json::json;

    fn setup_test_db() -> (FerridynDB, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let db = FerridynDB::create(dir.path().join("test.db")).unwrap();
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
        db.put_item("memories", json!({"category": "rust", "key": "ownership#borrowing", "content": "References allow borrowing without taking ownership"})).unwrap();
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
            db.put_item("memories", json!({"category": "bulk", "key": format!("item{i:02}"), "content": format!("c{i}")})).unwrap();
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
        db.put_item("memories", json!({"category": "test", "key": "with-meta", "content": "some content", "metadata": "tag:important"})).unwrap();
        let item = db
            .get_item("memories")
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
        let backend = MemoryBackend::Direct(db);
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
}
