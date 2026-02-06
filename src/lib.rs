//! FerridynDB Memory — shared library for MCP server and CLI.

pub mod backend;
pub mod error;
pub mod llm;
pub mod mcp;
pub mod schema;
pub mod ttl;

use std::path::PathBuf;

/// Default table name used for all memories (no namespace).
pub const TABLE_NAME: &str = "memories";

// Re-export server types for schema and index operations.
pub use ferridyn_server::client::{
    AttributeDefInput, AttributeInfo, IndexInfo, PartitionSchemaInfo, QueryResult,
};

// Re-export predefined schema types.
pub use schema::{PREDEFINED_SCHEMAS, PredefinedCategory, SchemaDefinition};

// Re-export TTL utilities.
pub use ttl::{
    INTERACTIONS_DEFAULT_TTL, SCRATCHPAD_DEFAULT_TTL, SESSIONS_DEFAULT_TTL, auto_ttl_from_date,
    compute_expires_at, filter_expired, is_expired, parse_ttl,
};

/// Resolve the table name from an optional namespace.
///
/// - `None` → `"memories"` (backward compatible default)
/// - `Some("myproject")` → `"memories_myproject"`
pub fn resolve_table_name(namespace: Option<&str>) -> String {
    match namespace {
        Some(ns) => format!("memories_{ns}"),
        None => TABLE_NAME.to_string(),
    }
}

/// Resolve the socket path from env var or default location.
pub fn resolve_socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("FERRIDYN_MEMORY_SOCKET") {
        return PathBuf::from(path);
    }

    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("ferridyn").join("server.sock")
}

/// Resolve the database path from env var or default location.
#[cfg(test)]
pub fn resolve_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("FERRIDYN_MEMORY_DB") {
        return PathBuf::from(path);
    }

    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("ferridyn").join("memory.db")
}

/// Open or create the database directly and ensure the memories table exists.
#[cfg(test)]
pub fn init_db_direct(
    path: &std::path::Path,
) -> Result<ferridyn_core::api::FerridynDB, Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = if path.exists() {
        ferridyn_core::api::FerridynDB::open(path)?
    } else {
        ferridyn_core::api::FerridynDB::create(path)?
    };

    ensure_memories_table_direct(&db, TABLE_NAME)?;
    Ok(db)
}

/// Create the memories table if it doesn't already exist (direct DB access).
#[cfg(test)]
fn ensure_memories_table_direct(
    db: &ferridyn_core::api::FerridynDB,
    table_name: &str,
) -> Result<(), ferridyn_core::error::Error> {
    use ferridyn_core::types::KeyType;

    match db
        .create_table(table_name)
        .partition_key("category", KeyType::String)
        .sort_key("key", KeyType::String)
        .execute()
    {
        Ok(()) => Ok(()),
        Err(ferridyn_core::error::Error::Schema(
            ferridyn_core::error::SchemaError::TableAlreadyExists(_),
        )) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Ensure the memories table exists via a server client.
pub async fn ensure_memories_table_via_server(
    client: &mut ferridyn_server::FerridynClient,
    table_name: &str,
) -> Result<(), ferridyn_server::error::ClientError> {
    use ferridyn_server::protocol::KeyDef;

    match client
        .create_table(
            table_name,
            KeyDef {
                name: "category".to_string(),
                key_type: "String".to_string(),
            },
            Some(KeyDef {
                name: "key".to_string(),
                key_type: "String".to_string(),
            }),
            None,
        )
        .await
    {
        Ok(()) => Ok(()),
        Err(ferridyn_server::error::ClientError::Server(ref e))
            if e.error == "TableAlreadyExists" =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}
