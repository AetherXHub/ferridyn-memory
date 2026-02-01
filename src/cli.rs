use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::Mutex;

use dynamite_memory::backend::MemoryBackend;
use dynamite_memory::{
    ensure_memories_table_via_server, init_db_direct, resolve_db_path, resolve_socket_path,
};

#[derive(Parser)]
#[command(
    name = "dynamite-memory-cli",
    about = "CLI for DynaMite memory operations"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Browse memory structure. Without --category: list all categories.
    /// With --category: list sort key prefixes within that category.
    Discover {
        #[arg(long)]
        category: Option<String>,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Retrieve memories by category, optionally filtered by sort key prefix.
    Recall {
        #[arg(long)]
        category: String,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Store a memory. Creates or replaces an existing entry.
    Remember {
        #[arg(long)]
        category: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        content: String,
        #[arg(long)]
        metadata: Option<String>,
    },
    /// Remove a specific memory by category and key.
    Forget {
        #[arg(long)]
        category: String,
        #[arg(long)]
        key: String,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let backend = connect_backend().await?;

    match cli.command {
        Command::Discover { category, limit } => {
            let items = if let Some(ref cat) = category {
                backend.list_sort_key_prefixes(cat, limit).await
            } else {
                backend.list_partition_keys(limit).await
            };
            let items = items.map_err(|e| format!("{}", e.message))?;
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        Command::Recall {
            category,
            prefix,
            limit,
        } => {
            let items = backend
                .query(&category, prefix.as_deref(), limit)
                .await
                .map_err(|e| format!("{}", e.message))?;
            println!("{}", serde_json::to_string_pretty(&items)?);
        }
        Command::Remember {
            category,
            key,
            content,
            metadata,
        } => {
            let mut doc = serde_json::json!({
                "category": category,
                "key": key,
                "content": content,
            });
            if let Some(ref m) = metadata {
                doc["metadata"] = serde_json::Value::String(m.clone());
            }
            backend
                .put_item(doc)
                .await
                .map_err(|e| format!("{}", e.message))?;
            eprintln!("Stored memory: {category}#{key}");
        }
        Command::Forget { category, key } => {
            backend
                .delete_item(&category, &key)
                .await
                .map_err(|e| format!("{}", e.message))?;
            eprintln!("Forgot memory: {category}#{key}");
        }
    }

    Ok(())
}

/// Try to connect to the dynamite-server socket. If it's not available,
/// fall back to opening the database directly.
async fn connect_backend() -> Result<MemoryBackend, Box<dyn std::error::Error>> {
    let socket_path = resolve_socket_path();

    if socket_path.exists() {
        match dynamite_server::DynaMiteClient::connect(&socket_path).await {
            Ok(mut client) => {
                ensure_memories_table_via_server(&mut client).await?;
                return Ok(MemoryBackend::Server(Arc::new(Mutex::new(client))));
            }
            Err(e) => {
                eprintln!(
                    "warning: server socket exists but connection failed ({e}), falling back to direct"
                );
            }
        }
    }

    let db_path = resolve_db_path();
    let db = init_db_direct(&db_path)?;
    Ok(MemoryBackend::Direct(db))
}
