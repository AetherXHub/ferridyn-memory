use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde_json::Value;
use tokio::sync::Mutex;

use ferridyn_memory::backend::MemoryBackend;
use ferridyn_memory::llm::{AnthropicClient, LlmClient};
use ferridyn_memory::schema::{
    NlIntent, PREDEFINED_SCHEMAS, ResolvedQuery, SchemaDefinition, SchemaManager, answer_query,
    classify_intent, parse_to_document, parse_to_document_with_category, resolve_query,
};
use ferridyn_memory::{PartitionSchemaInfo, ensure_memories_table_via_server, resolve_socket_path};

#[derive(Parser)]
#[command(
    name = "fmemory",
    about = "FerridynDB memory — store and recall persistent knowledge"
)]
struct Cli {
    /// Output machine-readable JSON (default: human-readable)
    #[arg(long, global = true)]
    json: bool,

    /// Natural language prompt (remember or recall via intent classification)
    #[arg(short, long)]
    prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Browse memory structure
    Discover {
        #[arg(long)]
        category: Option<String>,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Retrieve memories
    Recall {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        key: Option<String>,
        #[arg(long, help = "Natural language query")]
        query: Option<String>,
        #[arg(long, default_value = "20")]
        limit: usize,
    },
    /// Store a memory (NL-first)
    Remember {
        #[arg(long)]
        category: Option<String>,
        #[arg(long)]
        key: Option<String>,
        /// Natural language input (positional, collects remaining args)
        input: Vec<String>,
    },
    /// Remove a specific memory
    Forget {
        #[arg(long)]
        category: String,
        #[arg(long)]
        key: String,
    },
    /// Define a category schema with typed attributes
    Define {
        #[arg(long)]
        category: String,
        #[arg(long)]
        description: String,
        #[arg(
            long,
            help = "JSON array of attributes: [{\"name\":\"...\",\"type\":\"STRING\",\"required\":true}]"
        )]
        attributes: String,
        #[arg(long, help = "Auto-create indexes for suggested attributes")]
        auto_index: bool,
    },
    /// Show schema/index info
    Schema {
        #[arg(long)]
        category: Option<String>,
    },
    /// Initialize predefined categories and schemas
    Init {
        #[arg(long, help = "Recreate schemas even if they already exist")]
        force: bool,
    },
}

// ============================================================================
// Output Formatting
// ============================================================================

/// Format a single item for prose output.
/// Displays key (category) header then attributes with capitalized names.
fn format_item(item: &Value) {
    let key = item["key"].as_str().unwrap_or("?");
    let category = item["category"].as_str().unwrap_or("?");
    println!("{key} ({category})");

    if let Some(obj) = item.as_object() {
        for (attr_name, attr_value) in obj {
            if attr_name == "category" || attr_name == "key" {
                continue;
            }
            if attr_value.is_null() {
                continue;
            }
            let display_name = capitalize_first(attr_name);
            let display_value = match attr_value {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            println!("  {display_name}: {display_value}");
        }
    }
}

/// Format multiple items, separated by blank lines.
fn format_items(items: &[Value]) {
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            println!();
        }
        format_item(item);
    }
}

/// Capitalize the first letter of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let backend = connect_backend().await?;
    let schema_manager = SchemaManager::new(backend.clone());

    match cli.command {
        Some(Command::Discover { category, limit }) => {
            if let Some(ref cat) = category {
                // Show keys in category, attributes, and indexes.
                let items = backend
                    .query(cat, None, limit)
                    .await
                    .map_err(|e| e.to_string())?;
                let schema = schema_manager.get_schema(cat).await.ok().flatten();
                let indexes = schema_manager.list_indexes().await.unwrap_or_default();
                let cat_indexes: Vec<_> = indexes
                    .iter()
                    .filter(|idx| idx.partition_schema == *cat)
                    .collect();

                if cli.json {
                    let keys: Vec<&str> = items
                        .iter()
                        .filter_map(|item| item["key"].as_str())
                        .collect();
                    let output = serde_json::json!({
                        "category": cat,
                        "keys": keys,
                        "schema": schema.as_ref().map(|s| serde_json::json!({
                            "description": s.description,
                            "attributes": s.attributes.iter().map(|a| serde_json::json!({
                                "name": a.name,
                                "type": a.attr_type,
                                "required": a.required,
                            })).collect::<Vec<_>>(),
                        })),
                        "indexes": cat_indexes.iter().map(|idx| serde_json::json!({
                            "name": idx.name,
                            "attribute": idx.index_key_name,
                            "type": idx.index_key_type,
                        })).collect::<Vec<_>>(),
                    });
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    // Keys
                    let keys: Vec<&str> = items
                        .iter()
                        .filter_map(|item| item["key"].as_str())
                        .collect();
                    if keys.is_empty() {
                        eprintln!("No keys found in category '{cat}'.");
                    } else {
                        println!("Keys in {cat}:");
                        for key in &keys {
                            println!("  - {key}");
                        }
                    }

                    // Schema
                    if let Some(ref s) = schema {
                        println!();
                        println!("Schema: {}", s.description);
                        println!("Attributes:");
                        for attr in &s.attributes {
                            let req = if attr.required { ", required" } else { "" };
                            println!("  - {} ({}{})", attr.name, attr.attr_type, req);
                        }
                    }

                    // Indexes
                    if !cat_indexes.is_empty() {
                        println!("Indexes:");
                        for idx in &cat_indexes {
                            println!(
                                "  - {} ({}, {})",
                                idx.name, idx.index_key_name, idx.index_key_type
                            );
                        }
                    }
                }
            } else {
                // List all categories with schema descriptions and index counts.
                let schemas = schema_manager.list_schemas().await.unwrap_or_default();
                let indexes = schema_manager.list_indexes().await.unwrap_or_default();

                if cli.json {
                    let enriched: Vec<Value> = schemas
                        .iter()
                        .map(|s| {
                            let idx_count = indexes
                                .iter()
                                .filter(|idx| idx.partition_schema == s.prefix)
                                .count();
                            serde_json::json!({
                                "name": s.prefix,
                                "description": s.description,
                                "attribute_count": s.attributes.len(),
                                "index_count": idx_count,
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&enriched)?);
                } else if schemas.is_empty() {
                    eprintln!("No categories found.");
                } else {
                    for s in &schemas {
                        let idx_count = indexes
                            .iter()
                            .filter(|idx| idx.partition_schema == s.prefix)
                            .count();
                        println!(
                            "{}: {} ({} attributes, {} indexes)",
                            s.prefix,
                            s.description,
                            s.attributes.len(),
                            idx_count
                        );
                    }
                }
            }
        }
        Some(Command::Recall {
            category,
            key,
            query,
            limit,
        }) => {
            if let Some(ref cat) = category {
                if let Some(ref k) = key {
                    // Exact item by category + key.
                    let item = backend.get_item(cat, k).await.map_err(|e| e.to_string())?;
                    if let Some(item) = item {
                        if cli.json {
                            println!("{}", serde_json::to_string_pretty(&item)?);
                        } else {
                            format_item(&item);
                        }
                    } else {
                        eprintln!("No memory found for {cat}/{k}");
                    }
                } else {
                    // Scan category.
                    let items = backend
                        .query(cat, None, limit)
                        .await
                        .map_err(|e| e.to_string())?;
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&items)?);
                    } else if items.is_empty() {
                        eprintln!("No memories found in category '{cat}'.");
                    } else {
                        format_items(&items);
                    }
                }
            } else if let Some(ref q) = query {
                // NL query resolution.
                let llm = require_llm()?;
                let schemas = schema_manager
                    .list_schemas()
                    .await
                    .map_err(|e| e.to_string())?;
                if schemas.is_empty() {
                    eprintln!(
                        "No schemas defined. Use --category instead, or define schemas first."
                    );
                    std::process::exit(1);
                }
                let indexes = schema_manager.list_indexes().await.unwrap_or_default();

                let category_keys = fetch_category_keys(&backend, &schemas).await;
                let resolved = resolve_query(llm.as_ref(), &schemas, &indexes, &category_keys, q)
                    .await
                    .map_err(|e| format!("Query resolution failed: {e}"))?;

                let (items, _) = execute_with_fallback(&backend, &resolved, limit).await?;

                if cli.json {
                    println!("{}", serde_json::to_string_pretty(&items)?);
                } else if items.is_empty() {
                    eprintln!("No memories found.");
                } else {
                    match answer_query(llm.as_ref(), q, &items).await {
                        Ok(Some(answer)) => println!("{answer}"),
                        Ok(None) => eprintln!("No relevant memories found."),
                        Err(_) => {
                            // LLM synthesis failed — fall back to raw items.
                            format_items(&items);
                        }
                    }
                }
            } else {
                eprintln!("Either --category or --query is required.");
                std::process::exit(1);
            }
        }
        Some(Command::Remember {
            category,
            key,
            input,
        }) => {
            let input_text = input.join(" ");
            if input_text.is_empty() {
                eprintln!(
                    "Error: No input provided. Provide text to remember as positional arguments."
                );
                std::process::exit(1);
            }

            // Auto-init: ensure predefined schemas exist on first use.
            auto_init(&backend, &schema_manager).await?;

            let llm = require_llm()?;

            let (category, final_key, final_doc) = if let Some(cat) = category {
                // Category provided: validate it has a schema.
                if !schema_manager.has_schema(&cat).await.unwrap_or(false) {
                    let available: Vec<&str> = PREDEFINED_SCHEMAS.iter().map(|s| s.name).collect();
                    return Err(format!(
                        "Unknown category '{cat}'. Available: {}. \
                         Use `fmemory define` to create custom categories.",
                        available.join(", ")
                    )
                    .into());
                }
                let schema_info = schema_manager
                    .get_schema(&cat)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Schema for '{cat}' not found"))?;

                let doc = parse_to_document(llm.as_ref(), &cat, &schema_info, &input_text)
                    .await
                    .map_err(|e| format!("Document parsing failed: {e}"))?;
                let parsed_key = doc["key"].as_str().unwrap_or("unknown").to_string();
                let used_key = key.unwrap_or(parsed_key);
                (cat, used_key, doc)
            } else {
                // No category: let LLM pick from available schemas.
                let schemas = schema_manager.list_schemas().await.unwrap_or_default();
                let doc = parse_to_document_with_category(llm.as_ref(), &schemas, &input_text)
                    .await
                    .map_err(|e| format!("Document parsing failed: {e}"))?;
                let chosen_cat = doc["category"].as_str().unwrap_or("notes").to_string();
                let parsed_key = doc["key"].as_str().unwrap_or("unknown").to_string();
                let used_key = key.unwrap_or(parsed_key);
                (chosen_cat, used_key, doc)
            };

            // Build final document with category, key, and created_at.
            let mut final_item = serde_json::json!({
                "category": category,
                "key": final_key,
            });
            if let Some(obj) = final_doc.as_object() {
                for (k, v) in obj {
                    if k == "key" || k == "category" {
                        continue;
                    }
                    final_item[k] = v.clone();
                }
            }
            // Auto-inject created_at timestamp.
            final_item["created_at"] = Value::String(chrono::Utc::now().to_rfc3339());

            backend
                .put_item(final_item.clone())
                .await
                .map_err(|e| e.to_string())?;

            // Prose output: list non-null attribute names.
            let attr_names: Vec<&str> = final_item
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .filter(|(k, v)| {
                            *k != "category" && *k != "key" && *k != "created_at" && !v.is_null()
                        })
                        .map(|(k, _)| k.as_str())
                        .collect()
                })
                .unwrap_or_default();

            if attr_names.is_empty() {
                eprintln!("Stored {category}/{final_key}");
            } else {
                eprintln!("Stored {category}/{final_key} ({})", attr_names.join(", "));
            }
        }
        Some(Command::Forget { category, key }) => {
            backend
                .delete_item(&category, &key)
                .await
                .map_err(|e| e.to_string())?;
            eprintln!("Forgot: {category}/{key}");
        }
        Some(Command::Define {
            category,
            description,
            attributes,
            auto_index,
        }) => {
            let attr_defs: Vec<ferridyn_memory::schema::AttributeDef> =
                serde_json::from_str(&attributes)
                    .map_err(|e| format!("Invalid attributes JSON: {e}"))?;

            let suggested_indexes = if auto_index {
                attr_defs.iter().map(|a| a.name.clone()).collect()
            } else {
                vec![]
            };

            let definition = SchemaDefinition {
                description,
                attributes: attr_defs,
                suggested_indexes,
            };

            schema_manager
                .create_schema_with_indexes(&category, &definition, true)
                .await
                .map_err(|e| e.to_string())?;
            eprintln!("Schema defined for '{category}'");
        }
        Some(Command::Schema { category }) => {
            if let Some(ref cat) = category {
                let schema = schema_manager
                    .get_schema(cat)
                    .await
                    .map_err(|e| e.to_string())?;
                let indexes = schema_manager.list_indexes().await.unwrap_or_default();
                let cat_indexes: Vec<_> = indexes
                    .iter()
                    .filter(|idx| idx.partition_schema == *cat)
                    .collect();

                match schema {
                    Some(s) => {
                        if cli.json {
                            let output = serde_json::json!({
                                "category": cat,
                                "description": s.description,
                                "attributes": s.attributes.iter().map(|a| serde_json::json!({
                                    "name": a.name,
                                    "type": a.attr_type,
                                    "required": a.required,
                                })).collect::<Vec<_>>(),
                                "indexes": cat_indexes.iter().map(|idx| serde_json::json!({
                                    "name": idx.name,
                                    "attribute": idx.index_key_name,
                                    "type": idx.index_key_type,
                                })).collect::<Vec<_>>(),
                            });
                            println!("{}", serde_json::to_string_pretty(&output)?);
                        } else {
                            println!("Category: {cat}");
                            println!("Description: {}", s.description);
                            println!("Attributes:");
                            for attr in &s.attributes {
                                let req = if attr.required { ", required" } else { "" };
                                println!("  - {} ({}{})", attr.name, attr.attr_type, req);
                            }
                            if !cat_indexes.is_empty() {
                                println!("Indexes:");
                                for idx in &cat_indexes {
                                    println!(
                                        "  - {} ({}, {})",
                                        idx.name, idx.index_key_name, idx.index_key_type
                                    );
                                }
                            }
                        }
                    }
                    None => {
                        eprintln!("No schema defined for category '{cat}'");
                    }
                }
            } else {
                let schemas = schema_manager
                    .list_schemas()
                    .await
                    .map_err(|e| e.to_string())?;
                let indexes = schema_manager.list_indexes().await.unwrap_or_default();

                if schemas.is_empty() {
                    eprintln!("No schemas defined.");
                } else if cli.json {
                    let output: Vec<Value> = schemas
                        .iter()
                        .map(|s| {
                            let cat_indexes: Vec<_> = indexes
                                .iter()
                                .filter(|idx| idx.partition_schema == s.prefix)
                                .collect();
                            serde_json::json!({
                                "category": s.prefix,
                                "description": s.description,
                                "attributes": s.attributes.iter().map(|a| serde_json::json!({
                                    "name": a.name,
                                    "type": a.attr_type,
                                    "required": a.required,
                                })).collect::<Vec<_>>(),
                                "indexes": cat_indexes.iter().map(|idx| serde_json::json!({
                                    "name": idx.name,
                                    "attribute": idx.index_key_name,
                                    "type": idx.index_key_type,
                                })).collect::<Vec<_>>(),
                            })
                        })
                        .collect();
                    println!("{}", serde_json::to_string_pretty(&output)?);
                } else {
                    for s in &schemas {
                        let idx_count = indexes
                            .iter()
                            .filter(|idx| idx.partition_schema == s.prefix)
                            .count();
                        println!(
                            "{}: {} ({} attributes, {} indexes)",
                            s.prefix,
                            s.description,
                            s.attributes.len(),
                            idx_count
                        );
                    }
                }
            }
        }
        Some(Command::Init { force }) => {
            if force {
                // Drop and recreate all predefined schemas.
                for predefined in PREDEFINED_SCHEMAS {
                    let _ = backend.drop_schema(predefined.name).await;
                    // Also drop associated indexes.
                    let indexes = schema_manager.list_indexes().await.unwrap_or_default();
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
                .map_err(|e| e.to_string())?;

            if cli.json {
                let names: Vec<&str> = PREDEFINED_SCHEMAS.iter().map(|s| s.name).collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "initialized": names,
                    }))?
                );
            } else {
                eprintln!(
                    "Initialized {} predefined categories:",
                    PREDEFINED_SCHEMAS.len()
                );
                for s in PREDEFINED_SCHEMAS {
                    eprintln!("  - {}: {}", s.name, s.description);
                }
            }
        }
        None => {
            let input = match cli.prompt {
                Some(ref p) => p.clone(),
                None => {
                    Cli::parse_from(["fmemory", "--help"]);
                    return Ok(());
                }
            };

            let llm = require_llm().map_err(|e| {
                format!(
                    "{e}\n\n-p/--prompt requires ANTHROPIC_API_KEY. \
                     Use explicit subcommands (discover, recall, remember, ...) \
                     for API-key-free operation."
                )
            })?;

            // Auto-init predefined schemas.
            auto_init(&backend, &schema_manager).await?;

            // Classify intent: remember or recall.
            let intent = classify_intent(llm.as_ref(), &input)
                .await
                .map_err(|e| format!("Intent classification failed: {e}"))?;

            match intent {
                NlIntent::Remember { content } => {
                    // Let LLM pick category from available schemas.
                    let schemas = schema_manager.list_schemas().await.unwrap_or_default();
                    let doc = parse_to_document_with_category(llm.as_ref(), &schemas, &content)
                        .await
                        .map_err(|e| format!("Document parsing failed: {e}"))?;
                    let category = doc["category"].as_str().unwrap_or("notes").to_string();
                    let final_key = doc["key"].as_str().unwrap_or("unknown").to_string();

                    // Build final document with created_at.
                    let mut final_item = serde_json::json!({
                        "category": category,
                        "key": final_key,
                    });
                    if let Some(obj) = doc.as_object() {
                        for (k, v) in obj {
                            if k == "key" || k == "category" {
                                continue;
                            }
                            final_item[k] = v.clone();
                        }
                    }
                    final_item["created_at"] = Value::String(chrono::Utc::now().to_rfc3339());

                    backend
                        .put_item(final_item.clone())
                        .await
                        .map_err(|e| e.to_string())?;

                    // Output.
                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&final_item)?);
                    } else {
                        let attr_names: Vec<&str> = final_item
                            .as_object()
                            .map(|obj| {
                                obj.iter()
                                    .filter(|(k, v)| {
                                        *k != "category"
                                            && *k != "key"
                                            && *k != "created_at"
                                            && !v.is_null()
                                    })
                                    .map(|(k, _)| k.as_str())
                                    .collect()
                            })
                            .unwrap_or_default();

                        if attr_names.is_empty() {
                            eprintln!("Stored {category}/{final_key}");
                        } else {
                            eprintln!("Stored {category}/{final_key} ({})", attr_names.join(", "));
                        }
                    }
                }
                NlIntent::Recall { query } => {
                    // --- Recall flow (existing NL query resolution) ---
                    let schemas = schema_manager
                        .list_schemas()
                        .await
                        .map_err(|e| e.to_string())?;
                    if schemas.is_empty() {
                        eprintln!("No schemas defined yet. Run `fmemory init` first.");
                        std::process::exit(1);
                    }
                    let indexes = schema_manager.list_indexes().await.unwrap_or_default();

                    let category_keys = fetch_category_keys(&backend, &schemas).await;
                    let resolved =
                        resolve_query(llm.as_ref(), &schemas, &indexes, &category_keys, &query)
                            .await
                            .map_err(|e| format!("Query resolution failed: {e}"))?;

                    let (items, _) = execute_with_fallback(&backend, &resolved, 20).await?;

                    if cli.json {
                        println!("{}", serde_json::to_string_pretty(&items)?);
                    } else if items.is_empty() {
                        eprintln!("No memories found.");
                    } else {
                        match answer_query(llm.as_ref(), &query, &items).await {
                            Ok(Some(answer)) => println!("{answer}"),
                            Ok(None) => eprintln!("No relevant memories found."),
                            Err(_) => {
                                // LLM synthesis failed — fall back to raw items.
                                format_items(&items);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

// ============================================================================
// Resolved Query Execution
// ============================================================================

/// Execute a resolved query against the backend.
async fn execute_resolved_query(
    backend: &MemoryBackend,
    resolved: &ResolvedQuery,
    limit: usize,
) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    match resolved {
        ResolvedQuery::IndexLookup {
            index_name,
            key_value,
            ..
        } => {
            let items = backend
                .query_index(index_name, Value::String(key_value.clone()), Some(limit))
                .await
                .map_err(|e| e.to_string())?;
            Ok(items)
        }
        ResolvedQuery::PartitionScan {
            category,
            key_prefix,
        } => {
            let items = backend
                .query(category, key_prefix.as_deref(), limit)
                .await
                .map_err(|e| e.to_string())?;
            Ok(items)
        }
        ResolvedQuery::ExactLookup { category, key } => {
            let item = backend
                .get_item(category, key)
                .await
                .map_err(|e| e.to_string())?;
            Ok(item.into_iter().collect())
        }
    }
}

/// Execute a resolved query with broadening fallback.
///
/// If the initial query returns no results, falls back to scanning the entire
/// category. Returns `(items, is_fallback)`.
async fn execute_with_fallback(
    backend: &MemoryBackend,
    resolved: &ResolvedQuery,
    limit: usize,
) -> Result<(Vec<Value>, bool), Box<dyn std::error::Error>> {
    let items = execute_resolved_query(backend, resolved, limit).await?;
    if !items.is_empty() {
        return Ok((items, false));
    }

    // Already a full category scan — no broader fallback possible.
    if matches!(
        resolved,
        ResolvedQuery::PartitionScan {
            key_prefix: None,
            ..
        }
    ) {
        return Ok((items, false));
    }

    let category = resolved_category(resolved);
    let fallback_items = backend
        .query(category, None, limit)
        .await
        .map_err(|e| e.to_string())?;
    let has_results = !fallback_items.is_empty();
    Ok((fallback_items, has_results))
}

/// Extract the category from any resolved query variant.
fn resolved_category(resolved: &ResolvedQuery) -> &str {
    match resolved {
        ResolvedQuery::IndexLookup { category, .. }
        | ResolvedQuery::PartitionScan { category, .. }
        | ResolvedQuery::ExactLookup { category, .. } => category,
    }
}

// ============================================================================
// Helpers
// ============================================================================

/// Fetch a sample of sort keys for each category (for query resolution context).
async fn fetch_category_keys(
    backend: &MemoryBackend,
    schemas: &[PartitionSchemaInfo],
) -> Vec<(String, Vec<String>)> {
    let mut result = Vec::new();
    for schema in schemas {
        let keys = backend
            .list_sort_key_prefixes(&schema.prefix, 20)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        result.push((schema.prefix.clone(), keys));
    }
    result
}

/// Ensure predefined schemas exist. Called transparently on first use.
///
/// Only initializes if no schemas exist at all (first use of the database).
async fn auto_init(
    backend: &MemoryBackend,
    schema_manager: &SchemaManager,
) -> Result<(), Box<dyn std::error::Error>> {
    let schemas = schema_manager.list_schemas().await.unwrap_or_default();
    if schemas.is_empty() {
        backend
            .ensure_predefined_schemas()
            .await
            .map_err(|e| e.to_string())?;
        eprintln!(
            "Initialized {} predefined categories.",
            PREDEFINED_SCHEMAS.len()
        );
    }
    Ok(())
}

/// Create an LLM client from environment, or error if not available.
fn require_llm() -> Result<Arc<dyn LlmClient>, String> {
    let client = AnthropicClient::from_env()
        .map_err(|e| format!("{e}. Set ANTHROPIC_API_KEY for natural language queries."))?;
    Ok(Arc::new(client))
}

/// Connect to the ferridyn-server socket. Errors if the server is not available.
async fn connect_backend() -> Result<MemoryBackend, Box<dyn std::error::Error>> {
    let socket_path = resolve_socket_path();

    if !socket_path.exists() {
        return Err(format!(
            "ferridyn-server socket not found at {}. Start the server with: ferridyn-server",
            socket_path.display()
        )
        .into());
    }

    let mut client = ferridyn_server::FerridynClient::connect(&socket_path)
        .await
        .map_err(|e| {
            format!(
                "Failed to connect to ferridyn-server at {}: {e}",
                socket_path.display()
            )
        })?;
    ensure_memories_table_via_server(&mut client).await?;
    Ok(MemoryBackend::Server(Arc::new(Mutex::new(client))))
}
