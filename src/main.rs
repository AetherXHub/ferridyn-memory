use std::sync::Arc;

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tokio::sync::Mutex;

use ferridyn_memory::backend::MemoryBackend;
use ferridyn_memory::llm::AnthropicClient;
use ferridyn_memory::schema::SchemaStore;
use ferridyn_memory::{
    ensure_memories_table_via_server, init_db_direct, resolve_db_path, resolve_socket_path,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();

    // LLM client is required — fail early if missing.
    let llm = AnthropicClient::from_env().map_err(|e| {
        format!(
            "{e}. Set ANTHROPIC_API_KEY to enable schema inference and natural language recall."
        )
    })?;

    let backend = connect_backend().await?;
    let schema_store = SchemaStore::new(backend.clone());

    let server = ferridyn_memory::server::MemoryServer::new(backend, schema_store, Arc::new(llm));
    let service = server.serve(stdio()).await?;
    service.waiting().await?;

    Ok(())
}

/// Try to connect to the ferridyn-server socket. If it's not available,
/// fall back to opening the database directly.
async fn connect_backend() -> Result<MemoryBackend, Box<dyn std::error::Error>> {
    let socket_path = resolve_socket_path();

    // Try server connection first.
    if socket_path.exists() {
        match ferridyn_server::FerridynClient::connect(&socket_path).await {
            Ok(mut client) => {
                // Ensure the memories table exists on the server.
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

    // Fallback: open database directly.
    let db_path = resolve_db_path();
    let db = init_db_direct(&db_path)?;
    Ok(MemoryBackend::Direct(db))
}
