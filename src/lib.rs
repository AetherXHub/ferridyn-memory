//! FerridynDB Memory — shared library for MCP server and CLI.

pub mod backend;
pub mod error;
pub mod llm;
pub mod schema;

use std::path::PathBuf;

/// Table name used for all memories.
pub const TABLE_NAME: &str = "memories";

/// Resolve the socket path from env var or default location.
pub fn resolve_socket_path() -> PathBuf {
    if let Ok(path) = std::env::var("FERRIDYN_MEMORY_SOCKET") {
        return PathBuf::from(path);
    }

    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("ferridyn").join("server.sock")
}

/// Resolve the database path from env var or default location.
pub fn resolve_db_path() -> PathBuf {
    if let Ok(path) = std::env::var("FERRIDYN_MEMORY_DB") {
        return PathBuf::from(path);
    }

    let data_dir = dirs::data_local_dir().unwrap_or_else(|| PathBuf::from("."));
    data_dir.join("ferridyn").join("memory.db")
}

/// Open or create the database directly and ensure the memories table exists.
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

    ensure_memories_table_direct(&db)?;
    Ok(db)
}

/// Create the memories table if it doesn't already exist (direct DB access).
fn ensure_memories_table_direct(
    db: &ferridyn_core::api::FerridynDB,
) -> Result<(), ferridyn_core::error::Error> {
    use ferridyn_core::types::KeyType;

    match db
        .create_table(TABLE_NAME)
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
) -> Result<(), ferridyn_server::error::ClientError> {
    use ferridyn_server::protocol::KeyDef;

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
        Err(ferridyn_server::error::ClientError::Server(ref e))
            if e.error == "TableAlreadyExists" =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}
