use opencode_computer_use::mcp::{McpServer, ToolRegistry};
use opencode_computer_use::platform::Platform;
use opencode_computer_use::safety::{ConfirmationGate, RateLimiter, Sandbox, Severity};
use serde_json::Value;

/// Helper to run the server with known state and send a JSON-RPC message.
/// Returns the JSON-RPC response.
async fn send_rpc(method: &str, params: Option<Value>) -> Value {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params.unwrap_or(serde_json::json!({})),
    });

    let server = build_test_server().await;

    let line = serde_json::to_string(&request).unwrap();
    let response = server.handle_message(&line).await;

    match response {
        Some(resp) => serde_json::to_value(&resp).unwrap(),
        None => serde_json::json!({"jsonrpc":"2.0","id":null}),
    }
}

async fn build_test_server() -> McpServer {
    let platform = Platform::new().unwrap();
    let sandbox = Sandbox::new(false, vec![], vec![], vec![]);
    let gate = ConfirmationGate::new(false);
    let limiter = RateLimiter::new(false, 100, 1);

    let registry = ToolRegistry::new(
        platform,
        None,
        None,
        None,
        sandbox,
        gate,
        limiter,
        "test-session".into(),
    );

    McpServer::new(registry)
}

// ─── Health Tests ────────────────────────────────────────────

#[tokio::test]
async fn test_initialize() {
    let resp = send_rpc("initialize", None).await;
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["protocolVersion"].is_string());
    assert!(resp["result"]["capabilities"]["tools"].is_object());
    assert_eq!(resp["result"]["serverInfo"]["name"], "ja-opencode-computer-use");
}

#[tokio::test]
async fn test_tools_list() {
    let resp = send_rpc("tools/list", None).await;
    assert_eq!(resp["id"], 1);
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty(), "Should have at least one tool");

    // Check for specific tool names
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert!(names.contains(&"computer/screenshot"));
    assert!(names.contains(&"computer/click"));
    assert!(names.contains(&"computer/type"));
    assert!(names.contains(&"computer/key"));
    assert!(names.contains(&"computer/mouse_move"));
    assert!(names.contains(&"computer/scroll"));
    assert!(names.contains(&"computer/drag"));
    assert!(names.contains(&"computer/wait"));
    assert!(names.contains(&"a11y/tree"));
    assert!(names.contains(&"a11y/find"));
    assert!(names.contains(&"a11y/click"));
    assert!(names.contains(&"a11y/type"));
    assert!(names.contains(&"a11y/info"));
    assert!(names.contains(&"a11y/wait"));
    assert!(names.contains(&"llm/think"));
    assert!(names.contains(&"llm/describe"));
    assert!(names.contains(&"system/command"));
    assert!(names.contains(&"system/clipboard"));
    assert!(names.contains(&"system/windows"));
    assert!(names.contains(&"browser/open"));
    assert!(names.contains(&"browser/dom"));
    assert!(names.contains(&"browser/js"));
    assert!(names.contains(&"browser/tabs"));
    assert!(names.contains(&"vision/query"));
    assert!(names.contains(&"health/get"));
    assert!(names.contains(&"audit/recent"));
}

#[tokio::test]
async fn test_unknown_method() {
    let resp = send_rpc("foo/bar", None).await;
    assert_eq!(resp["error"]["code"], -32601);
}

#[tokio::test]
async fn test_invalid_json() {
    let server = build_test_server().await;
    let resp = server.handle_message("not valid json").await;
    assert!(resp.is_some());
    let resp = resp.unwrap();
    let val = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["error"]["code"], -32700);
}

#[tokio::test]
async fn test_tools_call_unknown() {
    let resp = send_rpc("tools/call", Some(serde_json::json!({
        "name": "nonexistent/tool",
        "arguments": {}
    }))).await;
    assert!(resp["error"]["message"].as_str().unwrap().contains("Tool not found"));
}

// ─── Safety Tests ────────────────────────────────────────────

#[tokio::test]
async fn test_health_get() {
    let resp = send_rpc("tools/call", Some(serde_json::json!({
        "name": "health/get",
        "arguments": {}
    }))).await;
    assert_eq!(resp["result"]["status"], "ok");
    assert!(resp["result"]["version"].is_string());
}

#[tokio::test]
async fn test_audit_recent() {
    let server = build_test_server().await;

    // Call health/get to create an audit entry
    let req1 = serde_json::json!({
        "jsonrpc": "2.0", "id": 1,
        "method": "tools/call",
        "params": {"name": "health/get", "arguments": {}}
    });
    server.handle_message(&serde_json::to_string(&req1).unwrap()).await;

    // Now call audit/recent
    let req2 = serde_json::json!({
        "jsonrpc": "2.0", "id": 2,
        "method": "tools/call",
        "params": {"name": "audit/recent", "arguments": {"count": 5}}
    });
    let resp = server.handle_message(&serde_json::to_string(&req2).unwrap()).await;
    let val = serde_json::to_value(&resp.unwrap()).unwrap();
    let entries = val["result"]["entries"].as_array().unwrap();
    assert!(!entries.is_empty(), "Audit log should have entries after health/get call");
    assert!(entries[0]["tool"].is_string());
    assert!(entries[0]["session_id"].is_string());
    assert!(entries[0]["duration_ms"].is_u64());
}

// ─── Sandbox Tests ──────────────────────────────────────────

#[tokio::test]
async fn test_sandbox_check_command_allowed() {
    let sandbox = Sandbox::new(true, vec![], vec![], vec!["git".into(), "ls".into()]);
    assert!(sandbox.check_command("git status").is_ok());
    assert!(sandbox.check_command("ls -la").is_ok());
}

#[tokio::test]
async fn test_sandbox_check_command_denied() {
    let sandbox = Sandbox::new(true, vec![], vec![], vec!["git".into()]);
    assert!(sandbox.check_command("rm -rf /").is_err());
    assert!(sandbox.check_command("curl http://evil.com").is_err());
}

#[tokio::test]
async fn test_sandbox_check_command_disabled() {
    let sandbox = Sandbox::new(false, vec![], vec![], vec![]);
    assert!(sandbox.check_command("rm -rf /").is_ok());
}

#[tokio::test]
async fn test_sandbox_network_allow() {
    let sandbox = Sandbox::new(true, vec![], vec!["*.github.com".into(), "api.openai.com".into()], vec![]);
    assert!(sandbox.check_network("api.github.com").is_ok());
    assert!(sandbox.check_network("api.openai.com").is_ok());
    assert!(sandbox.check_network("evil.com").is_err());
}

#[tokio::test]
async fn test_sandbox_path() {
    let sandbox = Sandbox::new(true, vec!["/home/user/projects".to_string()], vec![], vec![]);
    assert!(sandbox.check_path("/home/user/projects/foo").is_ok());
    assert!(sandbox.check_path("/etc/passwd").is_err());
}

// ─── Rate Limiter Tests ──────────────────────────────────────

#[tokio::test]
async fn test_rate_limiter() {
    let limiter = RateLimiter::new(true, 2, 5);
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_err());
}

#[tokio::test]
async fn test_rate_limiter_disabled() {
    let limiter = RateLimiter::new(false, 2, 5);
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_ok());
    assert!(limiter.check().await.is_ok());
}

// ─── Confirmation Gate Tests ─────────────────────────────────

#[tokio::test]
async fn test_confirmation_gate_disabled() {
    let gate = ConfirmationGate::new(false);
    assert!(gate.confirm("system/command", &serde_json::json!({"command": "rm -rf /"}), &Severity::Dangerous).await);
}

#[tokio::test]
async fn test_severity_classification() {
    let sandbox = Sandbox::new(false, vec![], vec![], vec![]);
    let gate = ConfirmationGate::new(false);
    let limiter = RateLimiter::new(false, 100, 1);
    let platform = Platform::new().unwrap();

    let registry = ToolRegistry::new(
        platform,
        None, None, None,
        sandbox, gate, limiter,
        "test".into(),
    );

    let result = registry.call("health/get", serde_json::json!({})).await;
    assert!(result.is_ok());
}

// ─── Session Isolation Tests ─────────────────────────────────

#[tokio::test]
async fn test_session_id_in_audit() {
    let sandbox = Sandbox::new(false, vec![], vec![], vec![]);
    let gate = ConfirmationGate::new(false);
    let limiter = RateLimiter::new(false, 100, 1);
    let platform = Platform::new().unwrap();

    let reg_a = ToolRegistry::new(
        platform, None, None, None,
        sandbox, gate, limiter,
        "session-a".into(),
    );

    let reg_b = reg_a.clone_with_session("session-b");

    reg_a.call("health/get", serde_json::json!({})).await.unwrap();
    reg_b.call("health/get", serde_json::json!({})).await.unwrap();

    let recent_a = reg_a.audit_log.recent(10).await;
    let recent_b = reg_b.audit_log.recent(10).await;

    assert!(recent_a.iter().all(|e| e.session_id == "session-a"),
        "All entries in session-a should have session-a id");
    assert!(recent_b.iter().all(|e| e.session_id == "session-b"),
        "All entries in session-b should have session-b id");
}

#[tokio::test]
async fn test_session_isolated_rate_limiters() {
    let sandbox = Sandbox::new(false, vec![], vec![], vec![]);
    let gate = ConfirmationGate::new(false);
    let limiter = RateLimiter::new(true, 1, 60);
    let platform = Platform::new().unwrap();

    let reg_a = ToolRegistry::new(
        platform, None, None, None,
        sandbox, gate, limiter,
        "session-a".into(),
    );

    let reg_b = reg_a.clone_with_session("session-b");

    assert!(reg_a.call("health/get", serde_json::json!({})).await.is_ok());
    assert!(reg_a.call("health/get", serde_json::json!({})).await.is_err());
    assert!(reg_b.call("health/get", serde_json::json!({})).await.is_ok());
}

#[tokio::test]
async fn test_tool_names() {
    let sandbox = Sandbox::new(false, vec![], vec![], vec![]);
    let gate = ConfirmationGate::new(false);
    let limiter = RateLimiter::new(false, 100, 1);
    let platform = Platform::new().unwrap();

    let registry = ToolRegistry::new(
        platform, None, None, None,
        sandbox, gate, limiter,
        "test".into(),
    );

    let names: Vec<&str> = registry.list().iter().map(|t| t.name).collect();
    assert!(names.contains(&"computer/screenshot"));
    assert!(names.contains(&"computer/click"));
    assert!(names.contains(&"computer/type"));
    assert!(names.contains(&"a11y/tree"));
    assert!(names.contains(&"a11y/find"));
    assert!(names.contains(&"a11y/click"));
    assert!(names.contains(&"a11y/type"));
    assert!(names.contains(&"a11y/info"));
    assert!(names.contains(&"a11y/wait"));
    assert!(names.contains(&"llm/think"));
    assert!(names.contains(&"system/command"));
    assert!(names.contains(&"system/clipboard"));
    assert!(names.contains(&"system/windows"));
    assert!(names.contains(&"browser/open"));
    assert!(names.contains(&"browser/dom"));
    assert!(names.contains(&"browser/js"));
    assert!(names.contains(&"browser/tabs"));
    assert!(names.contains(&"vision/query"));
    assert!(names.contains(&"health/get"));
    assert!(names.contains(&"audit/recent"));
}
