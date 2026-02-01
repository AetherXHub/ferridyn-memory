//! DynaMite Memory — shared library for MCP server and CLI.

pub mod backend;
pub mod error;
pub mod llm;
pub mod schema;
pub mod server;

use std::path::PathBuf;

/// Table name used for all memories.
pub const TABLE_NAME: &str = "memories";

/// Resolve the socket path from env var or default location.
pub fn resolve_socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("DYNAMITE_MEMORY_SOCKET") {
        return PathBuf::from(path);
    }

    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("dynamite").join("server.sock")
}

/// Resolve the database path from env var or default location.
pub fn resolve_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("DYNAMITE_MEMORY_DB") {
        return PathBuf::from(path);
    }

    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("dynamite").join("memory.db")
}

/// Open or create the database directly and ensure the memories table exists.
pub fn init_db_direct(
    path: &std::path::Path,
) -> Result<dynamite_core::api::DynaMite, Box<dyn std::error::Error>> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let db = if path.exists() {
        dynamite_core::api::DynaMite::open(path)?
    } else {
        dynamite_core::api::DynaMite::create(path)?
    };

    ensure_memories_table_direct(&db)?;
    Ok(db)
}

/// Create the memories table if it doesn't already exist (direct DB access).
fn ensure_memories_table_direct(
    db: &dynamite_core::api::DynaMite,
) -> Result<(), dynamite_core::error::Error> {
    use dynamite_core::types::KeyType;

    match db
        .create_table(TABLE_NAME)
        .partition_key("category", KeyType::String)
        .sort_key("key", KeyType::String)
        .execute()
    {
        Ok(()) => Ok(()),
        Err(dynamite_core::error::Error::Schema(
            dynamite_core::error::SchemaError::TableAlreadyExists(_),
        )) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Ensure the memories table exists via a server client.
pub async fn ensure_memories_table_via_server(
    client: &mut dynamite_server::DynaMiteClient,
) -> Result<(), dynamite_server::error::ClientError> {
    use dynamite_server::protocol::KeyDef;

    match client
        .create_table(
            TABLE_NAME,
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
        Err(dynamite_server::error::ClientError::Server(ref e))
            if e.error == "TableAlreadyExists" =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}
