use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::Mutex;

use dynamite_memory::backend::MemoryBackend;
use dynamite_memory::llm::{AnthropicClient, LlmClient};
use dynamite_memory::schema::{SchemaStore, resolve_query};
use dynamite_memory::{
    ensure_memories_table_via_server, init_db_direct, resolve_db_path, resolve_socket_path,
};

#[derive(Parser)]
#[command(
    name = "dynamite-memory-cli",
    about = "CLI for DynamiteDB memory operations"
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
    /// Retrieve memories by category or natural language query.
    Recall {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long, help = "Natural language query, e.g. \"Toby's email\"")]
        query: Option<String>,
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
    /// Define or update the schema for a memory category.
    Define {
        #[arg(long)]
        category: String,
        #[arg(long)]
        description: String,
        #[arg(long)]
        sort_key_format: String,
        #[arg(long, help = "JSON object: {\"segment\": \"description\", ...}")]
        segments: String,
        #[arg(long, help = "Comma-separated example keys")]
        examples: Option<String>,
    },
    /// Show schema for a category, or list all schemas.
    Schema {
        #[arg(long)]
        category: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let backend = connect_backend().await?;
    let schema_store = SchemaStore::new(backend.clone());

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
            query,
            limit,
        } => {
            let (resolved_category, resolved_prefix) = if let Some(ref q) = query {
                let llm = require_llm()?;
                let schemas = schema_store.list_schemas().await.map_err(|e| e.message)?;
                if schemas.is_empty() {
                    eprintln!(
                        "No schemas defined. Use --category instead, or define schemas first."
                    );
                    std::process::exit(1);
                }
                let (cat, pfx) = resolve_query(llm.as_ref(), &schemas, q)
                    .await
                    .map_err(|e| format!("Query resolution failed: {e}"))?;
                (cat, pfx)
            } else if let Some(ref cat) = category {
                (cat.clone(), prefix.clone())
            } else {
                eprintln!("Either --category or --query is required.");
                std::process::exit(1);
            };

            let items = backend
                .query(&resolved_category, resolved_prefix.as_deref(), limit)
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
        Command::Define {
            category,
            description,
            sort_key_format,
            segments,
            examples,
        } => {
            let segments_map: indexmap::IndexMap<String, String> = serde_json::from_str(&segments)
                .map_err(|e| format!("Invalid segments JSON: {e}"))?;

            let examples_vec: Vec<String> = examples
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let schema = dynamite_memory::schema::CategorySchema {
                description,
                sort_key_format,
                segments: segments_map,
                examples: examples_vec,
            };

            dynamite_memory::schema::validate_schema_format(&schema)
                .map_err(|e| format!("Invalid schema: {e}"))?;

            schema_store
                .put_schema(&category, &schema)
                .await
                .map_err(|e| format!("{}", e.message))?;
            eprintln!("Schema defined for category '{category}'");
        }
        Command::Schema { category } => {
            if let Some(ref cat) = category {
                match schema_store.get_schema(cat).await.map_err(|e| e.message)? {
                    Some(schema) => {
                        println!("{}", serde_json::to_string_pretty(&schema)?);
                    }
                    None => {
                        eprintln!("No schema defined for category '{cat}'");
                    }
                }
            } else {
                let schemas = schema_store.list_schemas().await.map_err(|e| e.message)?;
                if schemas.is_empty() {
                    eprintln!("No schemas defined.");
                } else {
                    for (name, schema) in &schemas {
                        println!(
                            "{name}: {} (key: {})",
                            schema.description, schema.sort_key_format
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

/// Create an LLM client from environment, or error if not available.
fn require_llm() -> Result<Arc<dyn LlmClient>, String> {
    let client = AnthropicClient::from_env()
        .map_err(|e| format!("{e}. Set ANTHROPIC_API_KEY for natural language queries."))?;
    Ok(Arc::new(client))
}

/// Try to connect to the dynamite-server socket. If it's not available,
/// fall back to opening the database directly.
async fn connect_backend() -> Result<MemoryBackend, Box<dyn std::error::Error>> {
    let socket_path = resolve_socket_path();

    if socket_path.exists() {
        match dynamite_server::DynamiteClient::connect(&socket_path).await {
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
