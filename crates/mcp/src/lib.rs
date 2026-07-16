//! MCP server for televisor — IPTV channel discovery and VLC playback for AI
//! agents, exposed as `televisor mcp`.

mod server;
mod tools;

pub use server::TelevisorMcp;

use rmcp::ServiceExt;
use rmcp::transport::stdio;

/// Options for the MCP server.
#[derive(Default)]
pub struct McpOptions {}

/// Serve MCP on stdio until the client disconnects.
///
/// The caller owns tracing setup and must send logs to **stderr** — stdout
/// carries the JSON-RPC stream (the televisor CLI does this for all commands).
pub async fn run_server(_opts: &McpOptions) -> Result<(), String> {
    tracing::info!("Starting televisor MCP server");

    let service = TelevisorMcp::new()
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("serving error: {e:?}"))
        .map_err(|e| e.to_string())?;

    service.waiting().await.map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_can_be_constructed() {
        let _server = TelevisorMcp::new();
        let _server = TelevisorMcp::default();
    }
}
