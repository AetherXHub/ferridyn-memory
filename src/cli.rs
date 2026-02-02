use std::sync::Arc;

use clap::{Parser, Subcommand};
use tokio::sync::Mutex;

use ferridyn_memory::backend::MemoryBackend;
use ferridyn_memory::llm::{AnthropicClient, LlmClient};
use ferridyn_memory::schema::{
    CategorySchema, SCHEMA_CATEGORY, SchemaStore, infer_schema, resolve_query,
};
use ferridyn_memory::{
    ensure_memories_table_via_server, init_db_direct, resolve_db_path, resolve_socket_path,
};

#[derive(Parser)]
#[command(
    name = "fmemory",
    about = "FerridynDB memory — store and recall persistent knowledge"
)]
struct Cli {
    /// Output machine-readable JSON (default: human-readable)
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Option<Command>,
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
        Some(Command::Discover { category, limit }) => {
            if let Some(ref cat) = category {
                // List sort key prefixes within a category.
                let items = backend
                    .list_sort_key_prefixes(cat, limit)
                    .await
                    .map_err(|e| e.to_string())?;
                let schema = schema_store.get_schema(cat).await.ok().flatten();

                if cli.json {
                    let output = serde_json::json!({
                        "prefixes": items,
                        "schema": schema,
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    if items.is_empty() {
                        eprintln!("No prefixes found in category '{cat}'.");
                    } else {
                        for item in &items {
                            if let Some(prefix) = item.as_str() {
                                println!("- {prefix}");
                            } else {
                                println!("- {item}");
                            }
                        }
                    }
                    if let Some(ref s) = schema {
                        eprintln!();
                        eprintln!("Schema: {} (key: {})", s.description, s.sort_key_format);
                    }
                }
            } else {
                // List all categories, excluding _schema.
                let items = backend
                    .list_partition_keys(limit + 1)
                    .await
                    .map_err(|e| e.to_string())?;

                let categories: Vec<String> = items
                    .iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .filter(|name| name != SCHEMA_CATEGORY)
                    .take(limit)
                    .collect();

                let schemas = schema_store.list_schemas().await.unwrap_or_default();
                let schema_map: std::collections::HashMap<&str, &CategorySchema> =
                    schemas.iter().map(|(name, s)| (name.as_str(), s)).collect();

                if cli.json {
                    let enriched: Vec<serde_json::Value> = categories
                        .iter()
                        .map(|name| {
                            if let Some(s) = schema_map.get(name.as_str()) {
                                serde_json::json!({
                                    "name": name,
                                    "description": s.description,
                                    "key_format": s.sort_key_format,
                                })
                            } else {
                                serde_json::json!({ "name": name })
                            }
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&enriched)?);
                } else if categories.is_empty() {
                    eprintln!("No categories found.");
                } else {
                    for name in &categories {
                        if let Some(s) = schema_map.get(name.as_str()) {
                            println!("- {name}: {} (key: {})", s.description, s.sort_key_format);
                        } else {
                            println!("- {name}");
                        }
                    }
                }
            }
        }
        Some(Command::Recall {
            category,
            prefix,
            query,
            limit,
        }) => {
            let (resolved_category, resolved_prefix) = if let Some(ref q) = query {
                let llm = require_llm()?;
                let schemas = schema_store
                    .list_schemas()
                    .await
                    .map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else if items.is_empty() {
                eprintln!("No memories found.");
            } else {
                if query.is_some() {
                    eprintln!(
                        "Resolved to: category={resolved_category}{}",
                        resolved_prefix
                            .as_ref()
                            .map(|p| format!(", prefix={p}"))
                            .unwrap_or_default()
                    );
                }
                for item in &items {
                    let key = item["key"].as_str().unwrap_or("?");
                    let content = item["content"].as_str().unwrap_or("");
                    let metadata = item["metadata"].as_str();
                    if let Some(meta) = metadata {
                        println!("[{key}]: {content} ({meta})");
                    } else {
                        println!("[{key}]: {content}");
                    }
                }
            }
        }
        Some(Command::Remember {
            category,
            key,
            content,
            metadata,
        }) => {
            // Reject writes to the _schema meta-category.
            if category == SCHEMA_CATEGORY {
                eprintln!(
                    "Error: Cannot write directly to the '{SCHEMA_CATEGORY}' category. Use 'define' instead."
                );
                std::process::exit(1);
            }

            // Schema validation or inference.
            let has_schema = schema_store.has_schema(&category).await.unwrap_or(false);
            if has_schema {
                if let Err(msg) = schema_store.validate_key(&category, &key).await {
                    eprintln!("Error: {msg}");
                    std::process::exit(1);
                }
            } else if let Some(llm) = try_llm()
                && let Some(schema) = infer_schema(llm.as_ref(), &category, &key, &content).await
            {
                if let Err(e) = schema_store.put_schema(&category, &schema).await {
                    eprintln!("warning: Failed to store inferred schema: {e}");
                } else {
                    eprintln!(
                        "Inferred schema for '{category}': {} (key: {})",
                        schema.description, schema.sort_key_format
                    );
                }
            }

            let mut doc = serde_json::json!({
                "category": category,
                "key": key,
                "content": content,
            });
            if let Some(ref m) = metadata {
                doc["metadata"] = serde_json::Value::String(m.clone());
            }
            backend.put_item(doc).await.map_err(|e| e.to_string())?;
            eprintln!("Stored: {category}#{key}");
        }
        Some(Command::Forget { category, key }) => {
            // Reject deletes from the _schema meta-category.
            if category == SCHEMA_CATEGORY {
                eprintln!("Error: Cannot delete from '{SCHEMA_CATEGORY}' directly.");
                std::process::exit(1);
            }

            backend
                .delete_item(&category, &key)
                .await
                .map_err(|e| e.to_string())?;
            eprintln!("Forgot: {category}#{key}");
        }
        Some(Command::Define {
            category,
            description,
            sort_key_format,
            segments,
            examples,
        }) => {
            let segments_map: indexmap::IndexMap<String, String> = serde_json::from_str(&segments)
                .map_err(|e| format!("Invalid segments JSON: {e}"))?;

            let examples_vec: Vec<String> = examples
                .as_deref()
                .unwrap_or("")
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let schema = CategorySchema {
                description,
                sort_key_format,
                segments: segments_map,
                examples: examples_vec,
            };

            ferridyn_memory::schema::validate_schema_format(&schema)
                .map_err(|e| format!("Invalid schema: {e}"))?;

            schema_store
                .put_schema(&category, &schema)
                .await
                .map_err(|e| e.to_string())?;
            eprintln!("Schema defined for '{category}'");
        }
        Some(Command::Schema { category }) => {
            if let Some(ref cat) = category {
                match schema_store
                    .get_schema(cat)
                    .await
                    .map_err(|e| e.to_string())?
                {
                    Some(schema) => {
                        if cli.json {
                            println!("{}", serde_json::to_string_pretty(&schema)?);
                        } else {
                            println!("Category: {cat}");
                            println!("Description: {}", schema.description);
                            println!("Key format: {}", schema.sort_key_format);
                            println!("Segments:");
                            for (name, desc) in &schema.segments {
                                println!("  {name}: {desc}");
                            }
                            if !schema.examples.is_empty() {
                                println!("Examples:");
                                for ex in &schema.examples {
                                    println!("  {ex}");
                                }
                            }
                        }
                    }
                    None => {
                        eprintln!("No schema defined for category '{cat}'");
                    }
                }
            } else {
                let schemas = schema_store
                    .list_schemas()
                    .await
                    .map_err(|e| e.to_string())?;
                if schemas.is_empty() {
                    eprintln!("No schemas defined.");
                } else if cli.json {
                    let map: serde_json::Map<String, serde_json::Value> = schemas
                        .iter()
                        .filter_map(|(name, schema)| {
                            serde_json::to_value(schema).ok().map(|v| (name.clone(), v))
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&map)?);
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
        None => {
            // NL-first mode: treat remaining args as a natural language query.
            let args: Vec<String> = std::env::args().skip(1).filter(|a| a != "--json").collect();

            if args.is_empty() {
                // No args at all -- show help.
                Cli::parse_from(["fmemory", "--help"]);
                return Ok(());
            }

            let query = args.join(" ");
            let llm = require_llm().map_err(|e| {
                format!(
                    "{e}\n\nNatural language mode requires ANTHROPIC_API_KEY. \
                     Use explicit subcommands (discover, recall, remember, ...) \
                     for API-key-free operation."
                )
            })?;

            let schemas = schema_store
                .list_schemas()
                .await
                .map_err(|e| e.to_string())?;
            if schemas.is_empty() {
                eprintln!(
                    "No schemas defined yet. Store some memories first, or use explicit subcommands."
                );
                std::process::exit(1);
            }

            let (category, prefix) = resolve_query(llm.as_ref(), &schemas, &query)
                .await
                .map_err(|e| format!("Query resolution failed: {e}"))?;

            let items = backend
                .query(&category, prefix.as_deref(), 20)
                .await
                .map_err(|e| e.to_string())?;

            if cli.json {
                println!("{}", serde_json::to_string_pretty(&items)?);
            } else if items.is_empty() {
                eprintln!("No memories found.");
            } else {
                eprintln!(
                    "Resolved to: category={category}{}",
                    prefix
                        .as_ref()
                        .map(|p| format!(", prefix={p}"))
                        .unwrap_or_default()
                );
                for item in &items {
                    let key = item["key"].as_str().unwrap_or("?");
                    let content = item["content"].as_str().unwrap_or("");
                    println!("[{key}]: {content}");
                }
            }
        }
    }

    Ok(())
}

/// Create an LLM client from environment, or return None if unavailable.
fn try_llm() -> Option<Arc<dyn LlmClient>> {
    AnthropicClient::from_env()
        .ok()
        .map(|c| Arc::new(c) as Arc<dyn LlmClient>)
}

/// Create an LLM client from environment, or error if not available.
fn require_llm() -> Result<Arc<dyn LlmClient>, String> {
    let client = AnthropicClient::from_env()
        .map_err(|e| format!("{e}. Set ANTHROPIC_API_KEY for natural language queries."))?;
    Ok(Arc::new(client))
}

/// Try to connect to the ferridyn-server socket. If it's not available,
/// fall back to opening the database directly.
async fn connect_backend() -> Result<MemoryBackend, Box<dyn std::error::Error>> {
    let socket_path = resolve_socket_path();

    if socket_path.exists() {
        match ferridyn_server::FerridynClient::connect(&socket_path).await {
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
