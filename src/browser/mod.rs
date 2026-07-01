//! Chrome DevTools Protocol (CDP) bridge.
//!
//! Connects to Chrome/Chromium/Brave/Edge via the remote debugging port,
//! sends CDP commands over WebSocket, and returns structured responses.
//!
//! Usage:
//!   1. Launch browser with: `google-chrome --remote-debugging-port=9222`
//!   2. This module connects to `http://localhost:9222/json/version`
//!   3. Gets the `webSocketDebuggerUrl` and opens a WebSocket connection
//!   4. Sends CDP commands like `Page.navigate`, `Runtime.evaluate`, etc.

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::Mutex;

/// CDP client that manages a WebSocket connection to Chrome's debugger.
pub struct CdpClient {
    ws: Mutex<Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>>>,
    debug_url: String,
    next_id: Mutex<u64>,
}

impl CdpClient {
    /// Connect to Chrome's debugging endpoint.
    /// `debug_url` is like `http://localhost:9222` or `http://127.0.0.1:9222`.
    pub async fn connect(debug_url: &str) -> Result<Self> {
        let base = debug_url.trim_end_matches('/');
        let info_url = format!("{}/json/version", base);

        let client = reqwest::Client::new();
        let resp: Value = client.get(&info_url).send().await?.json().await?;
        let ws_url = resp["webSocketDebuggerUrl"]
            .as_str()
            .context("Failed to get webSocketDebuggerUrl from Chrome. Is --remote-debugging-port set?")?
            .to_string();

        tracing::info!("CDP connected to Chrome at {}", ws_url);

        Ok(Self {
            ws: Mutex::new(None),
            debug_url: ws_url,
            next_id: Mutex::new(1),
        })
    }

    /// Ensure WebSocket is connected, reconnect if needed.
    async fn ensure_connected(&self) -> Result<()> {
        let mut guard = self.ws.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let (ws_stream, _) = tokio_tungstenite::connect_async(&self.debug_url)
            .await
            .context("Failed to connect to Chrome DevTools WebSocket")?;

        *guard = Some(ws_stream);
        Ok(())
    }

    /// Send a CDP command and wait for the response.
    /// `method` is like "Page.navigate", "Runtime.evaluate", etc.
    pub async fn send(&self, method: &str, params: Value) -> Result<Value> {
        self.ensure_connected().await?;

        let id = {
            let mut next = self.next_id.lock().await;
            let id = *next;
            *next += 1;
            id
        };

        let command = serde_json::json!({
            "id": id,
            "method": method,
            "params": params,
        });

        {
            let mut guard = self.ws.lock().await;
            let ws = guard.as_mut().unwrap();
            ws.send(tokio_tungstenite::tungstenite::Message::Text(
                serde_json::to_string(&command)?.into(),
            ))
            .await?;
        }

        // Read messages until we get the response for our ID
        loop {
            let mut guard = self.ws.lock().await;
            let ws = guard.as_mut().unwrap();

            match ws.next().await {
                Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                    let msg: Value = serde_json::from_str(&text)?;
                    if msg["id"].as_i64() == Some(id as i64) {
                        if let Some(error) = msg["error"].as_object() {
                            anyhow::bail!("CDP error ({}): {}",
                                error.get("code").and_then(|v| v.as_i64()).unwrap_or(-1),
                                error.get("message").and_then(|v| v.as_str()).unwrap_or("unknown"))
                        }
                        return Ok(msg["result"].clone());
                    }
                    // Check for errors in result
                    if let Some(result) = msg.get("result") {
                        if let Some(err) = result.get("exceptionDetails") {
                            anyhow::bail!("JS exception: {:?}", err);
                        }
                    }
                }
                Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                    // Connection closed, need to reconnect
                    *guard = None;
                    drop(guard);
                    // Try reconnecting
                    self.ensure_connected().await?;
                    // Resend command
                    return Box::pin(self.send(method, params)).await;
                }
                Some(Err(e)) => {
                    *guard = None;
                    anyhow::bail!("CDP WebSocket error: {}", e);
                }
                _ => {}
            }
        }
    }

    /// Navigate to a URL.
    pub async fn navigate(&self, url: &str) -> Result<Value> {
        self.send("Page.navigate", serde_json::json!({ "url": url })).await
    }

    /// Execute JavaScript in the page context and return the result.
    pub async fn evaluate(&self, js: &str) -> Result<Value> {
        self.send("Runtime.evaluate", serde_json::json!({
            "expression": js,
            "returnByValue": true,
            "awaitPromise": true,
        })).await
    }

    /// Get the full DOM document as a structured node.
    pub async fn get_document(&self) -> Result<Value> {
        self.send("DOM.getDocument", serde_json::json!({ "depth": -1 })).await
    }

    /// List all open targets (tabs, windows).
    pub async fn list_targets(&self) -> Result<Vec<Value>> {
        // We need to query Chrome via /json/list HTTP endpoint
        let base = self.debug_url.trim_end_matches('/');
        // Extract the host:port from the WS URL
        // ws://host:port/devtools/browser/... -> http://host:port
        let http_base = if let Some(rest) = base.strip_prefix("ws://") {
            let host_port = rest.split('/').next().unwrap_or("localhost:9222");
            format!("http://{}", host_port)
        } else {
            "http://localhost:9222".to_string()
        };

        let client = reqwest::Client::new();
        let resp: Vec<Value> = client.get(format!("{}/json/list", http_base)).send().await?.json().await?;
        Ok(resp)
    }

    /// Activate a target (tab) by its ID.
    pub async fn activate_target(&self, target_id: &str) -> Result<()> {
        // Also via HTTP endpoint
        let base = self.debug_url.trim_end_matches('/');
        let ws_prefix = if let Some(rest) = base.strip_prefix("ws://") {
            let host_port = rest.split('/').next().unwrap_or("localhost:9222");
            format!("http://{}", host_port)
        } else {
            "http://localhost:9222".to_string()
        };

        let client = reqwest::Client::new();
        client.get(format!("{}/json/activate/{}", ws_prefix, target_id)).send().await?;
        Ok(())
    }
}
