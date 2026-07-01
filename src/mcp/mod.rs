//! Minimal JSON-RPC 2.0 implementation for the Model Context Protocol.
//! MCP is JSON-RPC 2.0 over stdio with a specific message format:
//!   - requests: {"jsonrpc":"2.0","id":1,"method":"tools/call","params":{...}}
//!   - responses: {"jsonrpc":"2.0","id":1,"result":{...}}
//!   - notifications: {"jsonrpc":"2.0","method":"notifications/...","params":{...}}

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

mod tools;

pub use tools::{Tool, ToolRegistry};

/// A JSON-RPC request from the client
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// A JSON-RPC response to the client
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: Some(result), error: None }
    }

    pub fn error(id: Value, code: i32, message: String) -> Self {
        Self { jsonrpc: "2.0".into(), id, result: None, error: Some(JsonRpcError { code, message, data: None }) }
    }
}

/// A JSON-RPC notification (no response expected)
#[derive(Debug, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
}

/// The MCP server that reads JSON-RPC from stdin and writes to stdout.
pub struct McpServer {
    pub tool_registry: ToolRegistry,
}

impl McpServer {
    pub fn new(tool_registry: ToolRegistry) -> Self {
        Self { tool_registry }
    }

    /// Create a new McpServer with the same shared state but a different session ID.
    /// Used for SSE session isolation.
    pub fn clone_with_session(&self, session_id: &str) -> Self {
        Self {
            tool_registry: self.tool_registry.clone_with_session(session_id),
        }
    }

    /// Run the MCP server loop over stdio.
    /// Reads JSON-RPC lines from stdin, dispatches to handlers, writes responses to stdout.
    pub async fn run(&self) -> anyhow::Result<()> {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.trim().is_empty() {
                continue;
            }

            let response = self.handle_message(&line).await;

            if let Some(resp) = response {
                let json = serde_json::to_string(&resp)?;
                let mut stdout = tokio::io::stdout();
                stdout.write_all(json.as_bytes()).await?;
                stdout.write_all(b"\n").await?;
                stdout.flush().await?;
            }
        }

        Ok(())
    }

    /// Parse a JSON-RPC message string and dispatch to the appropriate handler.
    /// Returns `None` for notifications (no response expected).
    pub async fn handle_message(&self, line: &str) -> Option<JsonRpcResponse> {
        let msg: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                return Some(JsonRpcResponse::error(
                    Value::Null, -32700, format!("Parse error: {}", e),
                ));
            }
        };

        let method = msg["method"].as_str()?.to_string();
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method.as_str() {
            "initialize" => Some(self.handle_initialize(id.unwrap_or(Value::Null))),
            "tools/list" => Some(self.handle_tools_list(id.unwrap_or(Value::Null))),
            "tools/call" => {
                let tool_name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(Value::Null);
                Some(self.handle_tools_call(id.unwrap_or(Value::Null), tool_name, args).await)
            }
            "notifications/initialized" => None,
            "notifications/cancelled" => None,
            _ => Some(JsonRpcResponse::error(
                id.unwrap_or(Value::Null), -32601, format!("Method not found: {}", method),
            )),
        }
    }

    pub fn handle_initialize(&self, id: Value) -> JsonRpcResponse {
        let result = serde_json::json!({
            "protocolVersion": "0.1.0",
            "capabilities": {
                "tools": {
                    "listChanged": false
                }
            },
            "serverInfo": {
                "name": "ja-opencode-computer-use",
                "version": "0.1.0"
            }
        });
        JsonRpcResponse::success(id, result)
    }

    pub fn handle_tools_list(&self, id: Value) -> JsonRpcResponse {
        let tools: Vec<Value> = self.tool_registry.list().iter().map(|t| {
            serde_json::json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": t.input_schema,
            })
        }).collect();

        JsonRpcResponse::success(id, serde_json::json!({ "tools": tools }))
    }

    pub async fn handle_tools_call(&self, id: Value, tool_name: &str, args: Value) -> JsonRpcResponse {
        match self.tool_registry.call(tool_name, args).await {
            Ok(result) => JsonRpcResponse::success(id, result),
            Err(e) => JsonRpcResponse::error(id, -32603, format!("Tool error: {}", e)),
        }
    }
}
