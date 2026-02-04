use std::sync::Arc;

use clap::{Parser, Subcommand};
use serde_json::Value;
use tokio::sync::Mutex;

use ferridyn_memory::backend::MemoryBackend;
use ferridyn_memory::llm::{AnthropicClient, LlmClient};
use ferridyn_memory::schema::{
    NlIntent, ResolvedQuery, SchemaManager, answer_query, classify_intent, infer_schema,
    parse_to_document, resolve_query, strip_markdown_fences,
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
}

// ============================================================================
// Category Inference
// ============================================================================

const INFER_CATEGORY_PROMPT: &str = r#"Given a natural language input about something to remember, determine which category it belongs to.

Respond with ONLY a JSON object: {"category": "lowercase-name"}

Available categories (use one if appropriate, or suggest a new one):
"#;

async fn infer_category(
    llm: &dyn LlmClient,
    schemas: &[PartitionSchemaInfo],
    input: &str,
) -> Result<String, String> {
    let mut category_list = String::new();
    for schema in schemas {
        category_list.push_str(&format!("- {}: {}\n", schema.prefix, schema.description));
    }
    if category_list.is_empty() {
        category_list.push_str("(none yet — suggest a new category name)\n");
    }

    let system = format!("{INFER_CATEGORY_PROMPT}{category_list}");
    let completion = llm
        .complete(&system, input)
        .await
        .map_err(|e| format!("Category inference failed: {e}"))?;

    let cleaned = strip_markdown_fences(completion.text.trim());
    let parsed: Value =
        serde_json::from_str(&cleaned).map_err(|e| format!("Failed to parse category: {e}"))?;

    parsed["category"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "LLM response missing 'category' field".to_string())
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

            // Resolve category.
            let category = if let Some(cat) = category {
                cat
            } else {
                let llm = require_llm()?;
                let schemas = schema_manager.list_schemas().await.unwrap_or_default();
                infer_category(llm.as_ref(), &schemas, &input_text)
                    .await
                    .map_err(|e| format!("Category inference failed: {e}"))?
            };

            // Check if schema exists.
            let has_schema = schema_manager.has_schema(&category).await.unwrap_or(false);

            let (final_key, final_doc) = if !has_schema {
                // No schema yet: infer one.
                let llm = require_llm()?;
                let inferred = infer_schema(llm.as_ref(), &category, &input_text).await;
                if let Some(ref schema) = inferred {
                    if let Err(e) = schema_manager
                        .create_schema_with_indexes(&category, schema, false)
                        .await
                    {
                        eprintln!("warning: Failed to create inferred schema: {e}");
                    } else {
                        eprintln!("Inferred schema for '{}': {}", category, schema.description);
                    }
                }

                // After schema creation, try to parse with the new schema.
                let schema_info = schema_manager.get_schema(&category).await.ok().flatten();
                if let Some(ref info) = schema_info {
                    let doc = parse_to_document(llm.as_ref(), &category, info, &input_text)
                        .await
                        .map_err(|e| format!("Document parsing failed: {e}"))?;
                    let parsed_key = doc["key"].as_str().unwrap_or("unknown").to_string();
                    let used_key = key.unwrap_or(parsed_key);
                    (used_key, doc)
                } else {
                    // Schema creation failed; store as simple content.
                    let used_key = key.unwrap_or_else(|| "unknown".to_string());
                    let doc = serde_json::json!({
                        "content": input_text,
                    });
                    (used_key, doc)
                }
            } else {
                // Schema exists: parse to document.
                let llm = require_llm()?;
                let schema_info = schema_manager
                    .get_schema(&category)
                    .await
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("Schema for '{category}' not found"))?;

                let doc = parse_to_document(llm.as_ref(), &category, &schema_info, &input_text)
                    .await
                    .map_err(|e| format!("Document parsing failed: {e}"))?;
                let parsed_key = doc["key"].as_str().unwrap_or("unknown").to_string();
                let used_key = key.unwrap_or(parsed_key);
                (used_key, doc)
            };

            // Build final document: merge parsed doc with category and key.
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

            backend
                .put_item(final_item.clone())
                .await
                .map_err(|e| e.to_string())?;

            // Prose output: list non-null attribute names.
            let attr_names: Vec<&str> = final_item
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .filter(|(k, v)| *k != "category" && *k != "key" && !v.is_null())
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

            let inferred = ferridyn_memory::schema::InferredSchema {
                description,
                attributes: attr_defs,
                suggested_indexes,
            };

            schema_manager
                .create_schema_with_indexes(&category, &inferred, true)
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

            // Classify intent: remember or recall.
            let intent = classify_intent(llm.as_ref(), &input)
                .await
                .map_err(|e| format!("Intent classification failed: {e}"))?;

            match intent {
                NlIntent::Remember { content } => {
                    // --- Remember flow (mirrors the Remember subcommand) ---

                    // Infer category from content.
                    let schemas = schema_manager.list_schemas().await.unwrap_or_default();
                    let category = infer_category(llm.as_ref(), &schemas, &content)
                        .await
                        .map_err(|e| format!("Category inference failed: {e}"))?;

                    // Check if schema exists; infer one if not.
                    let has_schema = schema_manager.has_schema(&category).await.unwrap_or(false);

                    let (final_key, final_doc) = if !has_schema {
                        let inferred = infer_schema(llm.as_ref(), &category, &content).await;
                        if let Some(ref schema) = inferred {
                            if let Err(e) = schema_manager
                                .create_schema_with_indexes(&category, schema, false)
                                .await
                            {
                                eprintln!("warning: Failed to create inferred schema: {e}");
                            } else {
                                eprintln!(
                                    "Inferred schema for '{}': {}",
                                    category, schema.description
                                );
                            }
                        }

                        let schema_info = schema_manager.get_schema(&category).await.ok().flatten();
                        if let Some(ref info) = schema_info {
                            let doc = parse_to_document(llm.as_ref(), &category, info, &content)
                                .await
                                .map_err(|e| format!("Document parsing failed: {e}"))?;
                            let parsed_key = doc["key"].as_str().unwrap_or("unknown").to_string();
                            (parsed_key, doc)
                        } else {
                            let doc = serde_json::json!({ "content": content });
                            ("unknown".to_string(), doc)
                        }
                    } else {
                        let schema_info = schema_manager
                            .get_schema(&category)
                            .await
                            .map_err(|e| e.to_string())?
                            .ok_or_else(|| format!("Schema for '{category}' not found"))?;

                        let doc =
                            parse_to_document(llm.as_ref(), &category, &schema_info, &content)
                                .await
                                .map_err(|e| format!("Document parsing failed: {e}"))?;
                        let parsed_key = doc["key"].as_str().unwrap_or("unknown").to_string();
                        (parsed_key, doc)
                    };

                    // Build final document.
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
                                        *k != "category" && *k != "key" && !v.is_null()
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
                        eprintln!(
                            "No schemas defined yet. Store some memories first, \
                             or use explicit subcommands."
                        );
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
