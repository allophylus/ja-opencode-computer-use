use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{future, stream, StreamExt};
use serde_json::Value;
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tracing::info;
use uuid::Uuid;

use crate::mcp::McpServer;

struct SessionState {
    server: McpServer,
    #[allow(dead_code)]
    session_id: String,
    tx: broadcast::Sender<String>,
}

/// Shared state: a factory that creates per-session servers with unique session IDs.
struct AppState {
    server_template: McpServer,
    sessions: RwLock<HashMap<String, Arc<SessionState>>>,
}

/// Run the MCP server over SSE (Server-Sent Events) transport.
///
/// Each SSE connection gets a unique session ID for isolation.
/// POST /message/:session_id routes to the correct session.
pub async fn run_sse(server: McpServer, port: u16) -> anyhow::Result<()> {
    let state = Arc::new(AppState {
        server_template: server,
        sessions: RwLock::new(HashMap::new()),
    });

    let app = Router::new()
        .route("/sse", get(sse_handler))
        .route("/message/{session_id}", post(message_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("SSE transport listening on http://0.0.0.0:{}/sse", port);

    axum::serve(listener, app).await?;
    Ok(())
}

/// Handle SSE connection — creates a new session with a unique ID.
async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl futures_util::Stream<Item = Result<Event, Infallible>>> {
    let session_id = Uuid::new_v4().to_string();
    let (tx, _rx) = broadcast::channel(256);

    // Create a per-session McpServer with a unique session ID in the ToolRegistry
    let server = {
        let mut sessions = state.sessions.write().await;
        let session_state = Arc::new(SessionState {
            session_id: session_id.clone(),
            server: state.server_template.clone_with_session(&session_id),
            tx: tx.clone(),
        });
        sessions.insert(session_id.clone(), session_state);
        sessions.get(&session_id).cloned().unwrap()
    };

    info!("SSE session created: {}", session_id);

    let rx = server.tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(move |msg| {
        let event = match msg {
            Ok(json) => Some(Ok(Event::default().data(json))),
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(_)) => {
                Some(Ok(Event::default().data("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/sync\",\"params\":{\"dropped\":true}}")))
            }
        };
        future::ready(event)
    });

    let initial = stream::once(future::ready(
        Ok::<_, Infallible>(Event::default().event("endpoint").data(format!("/message/{}", session_id)))
    ));

    Sse::new(initial.chain(stream)).keep_alive(KeepAlive::default())
}

/// Handle incoming JSON-RPC message for a specific session.
async fn message_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let session = {
        let sessions = state.sessions.read().await;
        sessions.get(&session_id).cloned()
    };

    match session {
        Some(session) => {
            let line = serde_json::to_string(&body).unwrap_or_default();
            let response = session.server.handle_message(&line).await;

            if let Some(resp) = response {
                if let Ok(json_str) = serde_json::to_string(&resp) {
                    let _ = session.tx.send(json_str);
                }
                let json = serde_json::to_value(&resp).unwrap_or_default();
                Json(json)
            } else {
                Json(serde_json::json!({"jsonrpc":"2.0"}))
            }
        }
        None => Json(serde_json::json!({
            "jsonrpc": "2.0",
            "error": { "code": -32000, "message": format!("Session not found: {}", session_id) }
        })),
    }
}
