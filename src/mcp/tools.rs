use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::browser::CdpClient;
use crate::llm::LocalLlm;
use crate::platform::{self, Platform, ScreenCapture, InputSimulation, AccessibilityTree, SystemTools};
use crate::safety::{AuditLog, ConfirmationGate, RateLimiter, Sandbox, Severity};
use crate::vision::VisionAnalyser;

/// Context passed to every tool handler invocation.
pub struct McpContext<'a> {
    pub platform: &'a Platform,
    pub llm: Option<&'a LocalLlm>,
    pub browser: Option<&'a CdpClient>,
    pub vision: Option<&'a VisionAnalyser>,
    pub audit_log: Option<&'a AuditLog>,
    pub sandbox: Option<&'a Sandbox>,
    pub session_id: &'a str,
    pub args: Value,
}

/// A registered MCP tool
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub input_schema: Value,
    pub handler: Box<dyn ToolHandler + Send + Sync>,
}

#[async_trait]
pub trait ToolHandler: Send + Sync {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value>;
}

/// Registry of all available MCP tools.
/// Shared state (platform, llm, browser) is behind Arc for cheap cloning.
pub struct ToolRegistry {
    tools: Vec<Tool>,
    pub platform: Arc<Platform>,
    pub llm: Option<Arc<LocalLlm>>,
    pub browser: Option<Arc<CdpClient>>,
    pub vision: Option<Arc<VisionAnalyser>>,
    pub audit_log: AuditLog,
    pub sandbox: Arc<Sandbox>,
    pub confirmation_gate: Arc<ConfirmationGate>,
    pub rate_limiter: RateLimiter,
    pub session_id: String,
}

impl ToolRegistry {
    pub fn new(
        platform: Platform,
        llm: Option<LocalLlm>,
        browser: Option<CdpClient>,
        vision: Option<VisionAnalyser>,
        sandbox: Sandbox,
        confirmation_gate: ConfirmationGate,
        rate_limiter: RateLimiter,
        session_id: String,
    ) -> Self {
        let mut reg = Self {
            tools: vec![],
            platform: Arc::new(platform),
            llm: llm.map(Arc::new),
            browser: browser.map(Arc::new),
            vision: vision.map(Arc::new),
            audit_log: AuditLog::new(1000),
            sandbox: Arc::new(sandbox),
            confirmation_gate: Arc::new(confirmation_gate),
            rate_limiter,
            session_id,
        };
        reg.register_all();
        reg
    }

    /// Create a new registry sharing the same backend state but with an isolated session.
    /// Used by SSE transport for per-connection session isolation.
    pub fn clone_with_session(&self, session_id: &str) -> Self {
        let mut reg = Self {
            tools: vec![],
            platform: self.platform.clone(),
            llm: self.llm.clone(),
            browser: self.browser.clone(),
            vision: self.vision.clone(),
            audit_log: AuditLog::new(1000),
            sandbox: self.sandbox.clone(),
            confirmation_gate: self.confirmation_gate.clone(),
            rate_limiter: RateLimiter::new(true, 30, 1),
            session_id: session_id.to_string(),
        };
        reg.register_all();
        reg
    }

    fn register(&mut self, tool: Tool) {
        self.tools.push(tool);
    }

    pub async fn call(&self, name: &str, args: Value) -> anyhow::Result<Value> {
        // Rate limiting check
        self.rate_limiter.check().await.map_err(|e| anyhow::anyhow!("{}", e))?;

        // Find the tool
        let tool = self.tools.iter().find(|t| t.name == name)
            .ok_or_else(|| anyhow::anyhow!("Tool not found: {}", name))?;

        let severity = classify_severity(name, &args);

        // Sandbox checks
        self.check_sandbox(name, &args)?;

        // Confirmation gate
        if !self.confirmation_gate.confirm(name, &args, &severity).await {
            return Ok(serde_json::json!({ "cancelled": true, "reason": "User denied confirmation" }));
        }

        // Execute
        let start = std::time::Instant::now();
        let ctx = McpContext {
            platform: &*self.platform,
            llm: self.llm.as_deref(),
            browser: self.browser.as_deref(),
            vision: self.vision.as_deref(),
            audit_log: Some(&self.audit_log),
            sandbox: Some(&self.sandbox),
            session_id: &self.session_id,
            args: args.clone(),
        };
        let result = tool.handler.call(ctx).await;
        let duration_ms = start.elapsed().as_millis() as u64;

        // If a11y tool failed and vision fallback is available, retry via vision
        let result = match &result {
            Err(e) if name.starts_with("a11y/") && self.vision.is_some() => {
                tracing::info!("a11y tool '{}' failed ({}), falling back to vision", name, e);
                self.fallback_vision(name, &args).await
            }
            _ => result,
        };

        // Audit log
        let session_id = &self.session_id;
        match &result {
            Ok(val) => { self.audit_log.record(session_id, name, &args, val, duration_ms).await; }
            Err(e) => {
                let err_val = serde_json::json!({ "error": e.to_string() });
                self.audit_log.record(session_id, name, &args, &err_val, duration_ms).await;
            }
        }

        result
    }

    fn check_sandbox(&self, name: &str, args: &Value) -> anyhow::Result<()> {
        match name {
            "system/command" => {
                if let Some(cmd) = args["command"].as_str() {
                    self.sandbox.check_command(cmd)
                        .map_err(|e| anyhow::anyhow!("Sandbox: {}", e))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn fallback_vision(&self, _name: &str, _args: &Value) -> anyhow::Result<Value> {
        let vision = self.vision.as_ref().unwrap();
        let png = self.platform.capture_screen(None).await?;
        let result = vision.analyse(&png, None).await?;
        Ok(serde_json::to_value(&result)?)
    }

    /// List all registered tools
    pub fn list(&self) -> &[Tool] {
        self.tools.as_slice()
    }

    fn register_all(&mut self) {
        // ── Computer Tools ──────────────────────────────────────

        self.register(Tool {
            name: "computer/screenshot",
            description: "Capture the current screen as a base64-encoded PNG image. Use `scale` for Retina/HiDPI full-resolution capture.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "region": {
                        "type": "object",
                        "properties": {
                            "x": {"type": "integer"},
                            "y": {"type": "integer"},
                            "width": {"type": "integer"},
                            "height": {"type": "integer"}
                        }
                    },
                    "display": {"type": "integer"},
                    "scale": {"type": "number", "description": "Scale factor for HiDPI capture (e.g. 2.0 for Retina). Multiplies region coordinates by scale."}
                }
            }),
            handler: Box::new(ComputerScreenshot),
        });

        self.register(Tool {
            name: "computer/click",
            description: "Click the mouse at the specified screen coordinates",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["x", "y"],
                "properties": {
                    "x": {"type": "integer", "description": "X coordinate"},
                    "y": {"type": "integer", "description": "Y coordinate"},
                    "button": {"type": "string", "enum": ["left", "right", "middle"]},
                    "clicks": {"type": "integer", "default": 1}
                }
            }),
            handler: Box::new(ComputerClick),
        });

        self.register(Tool {
            name: "computer/mouse_move",
            description: "Move the mouse cursor to the specified coordinates",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["x", "y"],
                "properties": {
                    "x": {"type": "integer"},
                    "y": {"type": "integer"}
                }
            }),
            handler: Box::new(ComputerMouseMove),
        });

        self.register(Tool {
            name: "computer/type",
            description: "Type a string of text at the current focus",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["text"],
                "properties": {
                    "text": {"type": "string"}
                }
            }),
            handler: Box::new(ComputerType),
        });

        self.register(Tool {
            name: "computer/key",
            description: "Press a key or key combination",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["keys"],
                "properties": {
                    "keys": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "e.g. ['Cmd','S'], ['Ctrl','C']"
                    },
                    "duration_ms": {"type": "integer"}
                }
            }),
            handler: Box::new(ComputerKey),
        });

        self.register(Tool {
            name: "computer/scroll",
            description: "Scroll at the specified position",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["x", "y", "delta_x", "delta_y"],
                "properties": {
                    "x": {"type": "integer"},
                    "y": {"type": "integer"},
                    "delta_x": {"type": "integer"},
                    "delta_y": {"type": "integer"}
                }
            }),
            handler: Box::new(ComputerScroll),
        });

        self.register(Tool {
            name: "computer/drag",
            description: "Click and drag from one coordinate to another",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["start_x", "start_y", "end_x", "end_y"],
                "properties": {
                    "start_x": {"type": "integer"},
                    "start_y": {"type": "integer"},
                    "end_x": {"type": "integer"},
                    "end_y": {"type": "integer"}
                }
            }),
            handler: Box::new(ComputerDrag),
        });

        self.register(Tool {
            name: "computer/wait",
            description: "Wait for a specified duration in milliseconds",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["ms"],
                "properties": {
                    "ms": {"type": "integer"}
                }
            }),
            handler: Box::new(ComputerWait),
        });

        // ── Accessibility Tools ─────────────────────────────────

        self.register(Tool {
            name: "a11y/tree",
            description: "Get the full accessibility tree as structured JSON",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "depth": {"type": "integer"}
                }
            }),
            handler: Box::new(A11yTree),
        });

        self.register(Tool {
            name: "a11y/find",
            description: "Find UI elements by role, label, or text content",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "role": {"type": "string"},
                    "label": {"type": "string"},
                    "text": {"type": "string"}
                }
            }),
            handler: Box::new(A11yFind),
        });

        self.register(Tool {
            name: "a11y/click",
            description: "Click a UI element by its accessibility reference ID",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["ref_id"],
                "properties": {
                    "ref_id": {"type": "string"}
                }
            }),
            handler: Box::new(A11yClick),
        });

        self.register(Tool {
            name: "a11y/type",
            description: "Type text into a UI element by its accessibility reference ID",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["ref_id", "text"],
                "properties": {
                    "ref_id": {"type": "string"},
                    "text": {"type": "string"}
                }
            }),
            handler: Box::new(A11yType),
        });

        self.register(Tool {
            name: "a11y/info",
            description: "Get detailed information about a specific accessibility element",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["ref_id"],
                "properties": {
                    "ref_id": {"type": "string"}
                }
            }),
            handler: Box::new(A11yInfo),
        });

        self.register(Tool {
            name: "a11y/wait",
            description: "Wait for a UI element to appear, updating, or matching criteria. Polls accessibility tree until timeout.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "role": {"type": "string"},
                    "label": {"type": "string"},
                    "text": {"type": "string"},
                    "ref_id": {"type": "string"},
                    "timeout": {"type": "integer", "description": "Max wait time in seconds (default: 10)"},
                    "interval": {"type": "integer", "description": "Poll interval in ms (default: 500)"}
                }
            }),
            handler: Box::new(A11yWait),
        });

        // ── LLM Tools ──────────────────────────────────────────

        self.register(Tool {
            name: "llm/think",
            description: "Run text inference through the local LLM (useful for planning, reasoning, summarization)",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["prompt"],
                "properties": {
                    "prompt": {"type": "string", "description": "The prompt to send to the LLM"},
                    "model": {"type": "string", "description": "Override model name (default: llama3.2:3b)"}
                }
            }),
            handler: Box::new(LlmThink),
        });

        self.register(Tool {
            name: "llm/describe",
            description: "Capture a screenshot and analyse it with a local vision-language model",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "Optional prompt override (default: describe UI elements)"},
                    "model": {"type": "string", "description": "Vision model name (default: llama3.2-vision:11b)"}
                }
            }),
            handler: Box::new(LlmDescribe),
        });

        // ── System Tools ──────────────────────────────────────────

        self.register(Tool {
            name: "system/command",
            description: "Execute a shell command and capture stdout, stderr, and exit code",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["command"],
                "properties": {
                    "command": {"type": "string", "description": "Shell command to execute"},
                    "timeout": {"type": "integer", "description": "Timeout in seconds (default: 30)"}
                }
            }),
            handler: Box::new(SystemCommand),
        });

        self.register(Tool {
            name: "system/clipboard",
            description: "Read or write the system clipboard",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {"type": "string", "enum": ["read", "write"]},
                    "text": {"type": "string", "description": "Text to write (required for write action)"}
                }
            }),
            handler: Box::new(SystemClipboard),
        });

        self.register(Tool {
            name: "system/windows",
            description: "List, focus, or manage application windows",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["action"],
                "properties": {
                    "action": {"type": "string", "enum": ["list", "focus"]},
                    "window_id": {"type": "string", "description": "Window ID (required for focus action)"}
                }
            }),
            handler: Box::new(SystemWindows),
        });

        // ── Browser Tools ─────────────────────────────────────────

        self.register(Tool {
            name: "browser/open",
            description: "Navigate to a URL in the browser via Chrome DevTools Protocol",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["url"],
                "properties": {
                    "url": {"type": "string", "description": "URL to navigate to"},
                }
            }),
            handler: Box::new(BrowserOpen),
        });

        self.register(Tool {
            name: "browser/dom",
            description: "Get the full DOM document tree via Chrome DevTools Protocol",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "depth": {"type": "integer", "description": "DOM tree depth (-1 for full, default: -1)"}
                }
            }),
            handler: Box::new(BrowserDom),
        });

        self.register(Tool {
            name: "browser/js",
            description: "Execute JavaScript in the browser page and return the result",
            input_schema: serde_json::json!({
                "type": "object",
                "required": ["code"],
                "properties": {
                    "code": {"type": "string", "description": "JavaScript code to execute"},
                }
            }),
            handler: Box::new(BrowserJs),
        });

        self.register(Tool {
            name: "browser/tabs",
            description: "List all open browser tabs/windows",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "action": {"type": "string", "enum": ["list", "activate"], "description": "list tabs or activate a tab"},
                    "target_id": {"type": "string", "description": "Target ID to activate (required for activate action)"}
                }
            }),
            handler: Box::new(BrowserTabs),
        });

        // ── Vision Tools ──────────────────────────────────────────

        self.register(Tool {
            name: "vision/query",
            description: "Analyse a screenshot using a vision-language model. Returns detected UI elements with bounding boxes.",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "prompt": {"type": "string", "description": "Optional custom prompt for the VLM"},
                    "region": {"type": "object", "description": "Optional screen region to analyse {x, y, width, height}"}
                }
            }),
            handler: Box::new(VisionQuery),
        });

        // ── Health / Meta Tools ───────────────────────────────────

        self.register(Tool {
            name: "health/get",
            description: "Get server health status, including uptime and audit stats",
            input_schema: serde_json::json!({ "type": "object", "properties": {} }),
            handler: Box::new(HealthGet),
        });

        self.register(Tool {
            name: "audit/recent",
            description: "Get recent audit log entries",
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "count": {"type": "integer", "description": "Number of recent entries (default: 10)"}
                }
            }),
            handler: Box::new(AuditRecent),
        });
    }
}

// ─── Computer Tool Handlers ─────────────────────────────────

struct ComputerScreenshot;
#[async_trait]
impl ToolHandler for ComputerScreenshot {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let scale = ctx.args.get("scale").and_then(|v| v.as_f64()).unwrap_or(1.0);
        let region = ctx.args.get("region").map(|r| platform::Rect {
            x: (r["x"].as_f64().unwrap_or(0.0) * scale) as i32,
            y: (r["y"].as_f64().unwrap_or(0.0) * scale) as i32,
            width: (r["width"].as_f64().unwrap_or(0.0) * scale) as u32,
            height: (r["height"].as_f64().unwrap_or(0.0) * scale) as u32,
        });

        let png = match ctx.platform.capture_screen(region).await {
            Ok(png) => png,
            Err(_) if ctx.browser.is_some() => {
                // Fallback to CDP Page.captureScreenshot when native screenshot fails
                tracing::info!("Native screenshot failed, falling back to CDP capture");
                let result = ctx.browser.unwrap().send("Page.captureScreenshot", serde_json::json!({"format": "png"})).await?;
                let b64 = result["data"].as_str()
                    .ok_or_else(|| anyhow::anyhow!("CDP screenshot returned no data"))?;
                use base64::Engine;
                base64::engine::general_purpose::STANDARD.decode(b64)?
            }
            Err(e) => return Err(e),
        };

        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png);

        Ok(serde_json::json!({
            "type": "image",
            "data": b64,
            "mimeType": "image/png"
        }))
    }
}

struct ComputerClick;
#[async_trait]
impl ToolHandler for ComputerClick {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let x = ctx.args["x"].as_i64().unwrap_or(0) as i32;
        let y = ctx.args["y"].as_i64().unwrap_or(0) as i32;
        let button = match ctx.args["button"].as_str() {
            Some("right") => platform::MouseButton::Right,
            Some("middle") => platform::MouseButton::Middle,
            _ => platform::MouseButton::Left,
        };
        let clicks = ctx.args["clicks"].as_i64().unwrap_or(1) as u32;

        ctx.platform.mouse_click(x, y, button, clicks).await?;
        Ok(serde_json::json!({"success": true, "action": "click", "x": x, "y": y}))
    }
}

struct ComputerMouseMove;
#[async_trait]
impl ToolHandler for ComputerMouseMove {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let x = ctx.args["x"].as_i64().unwrap_or(0) as i32;
        let y = ctx.args["y"].as_i64().unwrap_or(0) as i32;
        ctx.platform.mouse_move(x, y).await?;
        Ok(serde_json::json!({"success": true, "action": "mouse_move", "x": x, "y": y}))
    }
}

struct ComputerType;
#[async_trait]
impl ToolHandler for ComputerType {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let text = ctx.args["text"].as_str().unwrap_or("");
        ctx.platform.type_text(text).await?;
        Ok(serde_json::json!({"success": true, "action": "type", "chars": text.len()}))
    }
}

struct ComputerKey;
#[async_trait]
impl ToolHandler for ComputerKey {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let keys: Vec<platform::Key> = ctx.args["keys"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|k| k.as_str())
                    .map(|k| {
                        if k.len() == 1 {
                            platform::Key::Char(k.chars().next().unwrap())
                        } else {
                            platform::Key::Named(k.to_string())
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let duration_ms = ctx.args["duration_ms"].as_i64().map(|d| d as u64);
        ctx.platform.key_press(&keys, duration_ms).await?;
        Ok(serde_json::json!({"success": true, "action": "key_press"}))
    }
}

struct ComputerScroll;
#[async_trait]
impl ToolHandler for ComputerScroll {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let x = ctx.args["x"].as_i64().unwrap_or(0) as i32;
        let y = ctx.args["y"].as_i64().unwrap_or(0) as i32;
        let dx = ctx.args["delta_x"].as_i64().unwrap_or(0) as i32;
        let dy = ctx.args["delta_y"].as_i64().unwrap_or(0) as i32;
        ctx.platform.mouse_scroll(x, y, dx, dy).await?;
        Ok(serde_json::json!({"success": true, "action": "scroll"}))
    }
}

struct ComputerDrag;
#[async_trait]
impl ToolHandler for ComputerDrag {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let sx = ctx.args["start_x"].as_i64().unwrap_or(0) as i32;
        let sy = ctx.args["start_y"].as_i64().unwrap_or(0) as i32;
        let ex = ctx.args["end_x"].as_i64().unwrap_or(0) as i32;
        let ey = ctx.args["end_y"].as_i64().unwrap_or(0) as i32;
        ctx.platform.mouse_drag((sx, sy), (ex, ey)).await?;
        Ok(serde_json::json!({"success": true, "action": "drag"}))
    }
}

struct ComputerWait;
#[async_trait]
impl ToolHandler for ComputerWait {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let ms = ctx.args["ms"].as_i64().unwrap_or(1000) as u64;
        tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
        Ok(serde_json::json!({"success": true, "action": "wait", "ms": ms}))
    }
}

// ─── Accessibility Tool Handlers ────────────────────────────

struct A11yTree;
#[async_trait]
impl ToolHandler for A11yTree {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let depth = ctx.args.get("depth").and_then(|d| d.as_i64()).map(|d| d as u32);
        let tree = ctx.platform.get_tree(depth).await?;
        Ok(serde_json::to_value(tree)?)
    }
}

struct A11yFind;
#[async_trait]
impl ToolHandler for A11yFind {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let criteria = platform::FindCriteria {
            role: ctx.args.get("role").and_then(|v| v.as_str().map(String::from)),
            label: ctx.args.get("label").and_then(|v| v.as_str().map(String::from)),
            text: ctx.args.get("text").and_then(|v| v.as_str().map(String::from)),
        };
        let results = ctx.platform.find_element(&criteria).await?;
        Ok(serde_json::to_value(results)?)
    }
}

struct A11yClick;
#[async_trait]
impl ToolHandler for A11yClick {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let ref_id = ctx.args["ref_id"].as_str().unwrap_or("");
        ctx.platform.click_element(ref_id).await?;
        Ok(serde_json::json!({"success": true, "action": "a11y_click", "ref": ref_id}))
    }
}

struct A11yType;
#[async_trait]
impl ToolHandler for A11yType {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let ref_id = ctx.args["ref_id"].as_str().unwrap_or("");
        let text = ctx.args["text"].as_str().unwrap_or("");
        ctx.platform.type_into_element(ref_id, text).await?;
        Ok(serde_json::json!({"success": true, "action": "a11y_type", "ref": ref_id}))
    }
}

struct A11yInfo;
#[async_trait]
impl ToolHandler for A11yInfo {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let ref_id = ctx.args["ref_id"].as_str().ok_or_else(|| anyhow::anyhow!("'ref_id' is required"))?;
        let info = ctx.platform.get_element_info(ref_id).await?;
        Ok(serde_json::to_value(info)?)
    }
}

struct A11yWait;
#[async_trait]
impl ToolHandler for A11yWait {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let timeout_s = ctx.args.get("timeout").and_then(|v| v.as_i64()).unwrap_or(10);
        let interval_ms = ctx.args.get("interval").and_then(|v| v.as_i64()).unwrap_or(500);
        let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(timeout_s as u64);
        let criteria = platform::FindCriteria {
            role: ctx.args.get("role").and_then(|v| v.as_str().map(String::from)),
            label: ctx.args.get("label").and_then(|v| v.as_str().map(String::from)),
            text: ctx.args.get("text").and_then(|v| v.as_str().map(String::from)),
        };
        let ref_id = ctx.args.get("ref_id").and_then(|v| v.as_str().map(String::from));

        while tokio::time::Instant::now() < deadline {
            if let Some(ref rid) = ref_id {
                if ctx.platform.get_element_info(rid).await.is_ok() {
                    return Ok(serde_json::json!({ "found": true, "ref_id": rid, "method": "by_ref" }));
                }
            } else {
                let results = ctx.platform.find_element(&criteria).await?;
                if !results.is_empty() {
                    return Ok(serde_json::json!({ "found": true, "elements": results }));
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(interval_ms as u64)).await;
        }

        Err(anyhow::anyhow!("Timeout after {}s waiting for element", timeout_s))
    }
}

// ─── LLM Tool Handlers ─────────────────────────────────────

struct LlmThink;
#[async_trait]
impl ToolHandler for LlmThink {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let prompt = ctx.args["prompt"].as_str().ok_or_else(|| anyhow::anyhow!("'prompt' is required"))?;
        let model = ctx.args.get("model").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("llama3.2:3b");
        let llm = ctx.llm.ok_or_else(|| anyhow::anyhow!("Local LLM not enabled. Set llm.enabled = true in config"))?;

        let cfg = crate::llm::LlmConfig { text_model: model.into(), ..Default::default() };
        let result = llm.generate(model, prompt, &cfg).await?;
        Ok(serde_json::json!({ "response": result.text, "tokens_used": result.tokens_used, "provider": result.provider }))
    }
}

struct LlmDescribe;
#[async_trait]
impl ToolHandler for LlmDescribe {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let prompt = ctx.args.get("prompt").and_then(|v| v.as_str()).filter(|s| !s.is_empty())
            .unwrap_or("Describe what you see in this screenshot. List all UI elements, their positions, and possible functions.");
        let model = ctx.args.get("model").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).unwrap_or("llama3.2-vision:11b");
        let llm = ctx.llm.ok_or_else(|| anyhow::anyhow!("Local LLM not enabled. Set llm.enabled = true in config"))?;

        let png = ctx.platform.capture_screen(None).await?;
        let cfg = crate::llm::LlmConfig { vision_model: model.into(), ..Default::default() };
        let result = llm.analyze_image(model, prompt, &png, &cfg).await?;
        Ok(serde_json::json!({ "response": result.text, "tokens_used": result.tokens_used, "provider": result.provider }))
    }
}

// ─── System Tool Handlers ──────────────────────────────────

struct SystemCommand;
#[async_trait]
impl ToolHandler for SystemCommand {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let command = ctx.args["command"].as_str().ok_or_else(|| anyhow::anyhow!("'command' is required"))?;
        let timeout = ctx.args.get("timeout").and_then(|v| v.as_i64()).unwrap_or(30) as u64;

        let result = ctx.platform.run_command(command, timeout).await?;
        Ok(serde_json::json!({
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
        }))
    }
}

// ─── Browser Tool Handlers ──────────────────────────────────

struct BrowserOpen;
#[async_trait]
impl ToolHandler for BrowserOpen {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let url = ctx.args["url"].as_str().ok_or_else(|| anyhow::anyhow!("'url' is required"))?;
        let cdp = ctx.browser.ok_or_else(|| anyhow::anyhow!("Browser CDP not enabled. Set browser.enabled = true and launch Chrome with --remote-debugging-port=9222"))?;
        let result = cdp.navigate(url).await?;
        Ok(serde_json::json!({ "success": true, "url": url, "result": result }))
    }
}

struct BrowserDom;
#[async_trait]
impl ToolHandler for BrowserDom {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let cdp = ctx.browser.ok_or_else(|| anyhow::anyhow!("Browser CDP not enabled"))?;
        let depth = ctx.args.get("depth").and_then(|v| v.as_i64()).unwrap_or(-1);
        let result = cdp.send("DOM.getDocument", serde_json::json!({ "depth": depth })).await?;
        Ok(result)
    }
}

struct BrowserJs;
#[async_trait]
impl ToolHandler for BrowserJs {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let code = ctx.args["code"].as_str().ok_or_else(|| anyhow::anyhow!("'code' is required"))?;
        let cdp = ctx.browser.ok_or_else(|| anyhow::anyhow!("Browser CDP not enabled"))?;
        let result = cdp.evaluate(code).await?;
        Ok(serde_json::json!({ "result": result }))
    }
}

struct BrowserTabs;
#[async_trait]
impl ToolHandler for BrowserTabs {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let cdp = ctx.browser.ok_or_else(|| anyhow::anyhow!("Browser CDP not enabled"))?;
        let action = ctx.args.get("action").and_then(|v| v.as_str()).unwrap_or("list");

        match action {
            "list" => {
                let targets = cdp.list_targets().await?;
                Ok(serde_json::json!({ "targets": targets }))
            }
            "activate" => {
                let target_id = ctx.args["target_id"].as_str().ok_or_else(|| anyhow::anyhow!("'target_id' is required for activate"))?;
                cdp.activate_target(target_id).await?;
                Ok(serde_json::json!({ "success": true, "action": "activate", "target_id": target_id }))
            }
            _ => Err(anyhow::anyhow!("Unknown tabs action: {}", action)),
        }
    }
}

struct SystemClipboard;
#[async_trait]
impl ToolHandler for SystemClipboard {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let action = ctx.args["action"].as_str().ok_or_else(|| anyhow::anyhow!("'action' is required (read/write)"))?;

        match action {
            "read" => {
                let text = ctx.platform.clipboard_get().await?;
                Ok(serde_json::json!({ "text": text }))
            }
            "write" => {
                let text = ctx.args["text"].as_str().ok_or_else(|| anyhow::anyhow!("'text' is required for write action"))?;
                ctx.platform.clipboard_set(text).await?;
                Ok(serde_json::json!({ "success": true, "action": "clipboard_write" }))
            }
            _ => Err(anyhow::anyhow!("Unknown clipboard action: {}", action)),
        }
    }
}

struct SystemWindows;
#[async_trait]
impl ToolHandler for SystemWindows {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let action = ctx.args["action"].as_str().ok_or_else(|| anyhow::anyhow!("'action' is required (list/focus)"))?;

        match action {
            "list" => {
                let windows = ctx.platform.list_windows().await?;
                Ok(serde_json::json!({ "windows": windows }))
            }
            "focus" => {
                let window_id = ctx.args["window_id"].as_str().ok_or_else(|| anyhow::anyhow!("'window_id' is required for focus action"))?;
                ctx.platform.focus_window(window_id).await?;
                Ok(serde_json::json!({ "success": true, "action": "focus_window", "window_id": window_id }))
            }
            _ => Err(anyhow::anyhow!("Unknown window action: {}", action)),
        }
    }
}

// ─── Vision Tool Handlers ────────────────────────────────────

struct VisionQuery;
#[async_trait]
impl ToolHandler for VisionQuery {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let vision = ctx.vision.ok_or_else(|| anyhow::anyhow!("Vision fallback not enabled. Set vision.enabled = true in config"))?;
        let region = ctx.args.get("region").map(|r| crate::platform::Rect {
            x: r["x"].as_i64().unwrap_or(0) as i32,
            y: r["y"].as_i64().unwrap_or(0) as i32,
            width: r["width"].as_i64().unwrap_or(0) as u32,
            height: r["height"].as_i64().unwrap_or(0) as u32,
        });
        let png = ctx.platform.capture_screen(region).await?;
        let prompt = ctx.args.get("prompt").and_then(|v| v.as_str());
        let result = vision.analyse(&png, prompt).await?;
        Ok(serde_json::to_value(&result)?)
    }
}

// ─── Health / Meta Tool Handlers ─────────────────────────────

struct HealthGet;
#[async_trait]
impl ToolHandler for HealthGet {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        Ok(serde_json::json!({
            "status": "ok",
            "version": env!("CARGO_PKG_VERSION"),
            "llm_enabled": ctx.llm.is_some(),
            "browser_enabled": ctx.browser.is_some(),
            "vision_enabled": ctx.vision.is_some(),
        }))
    }
}

struct AuditRecent;
#[async_trait]
impl ToolHandler for AuditRecent {
    async fn call(&self, ctx: McpContext<'_>) -> anyhow::Result<Value> {
        let count = ctx.args.get("count").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
        let entries = ctx.audit_log
            .ok_or_else(|| anyhow::anyhow!("Audit log not available"))?
            .recent(count).await;
        Ok(serde_json::json!({ "entries": entries }))
    }
}

/// Classify the severity of a tool action for confirmation gating.
fn classify_severity(name: &str, _args: &Value) -> Severity {
    match name {
        "system/command" => Severity::Dangerous,
        "system/clipboard" => Severity::Dangerous,
        "browser/js" => Severity::Normal,
        "browser/open" => Severity::Normal,
        "computer/click" | "computer/drag" | "computer/type" | "computer/key" => Severity::Normal,
        "a11y/click" | "a11y/type" => Severity::Normal,
        _ => Severity::Info,
    }
}
