# ja-opencode-computer-use — Specification Document

## 1. Overview

**ja-opencode-computer-use** is an MCP (Model Context Protocol) server that gives AI coding agents (opencode, Claude Code, Cursor, etc.) the ability to control desktop applications and web browsers on the host machine. It's a cross-platform re-implementation inspired by Anthropic's Computer Use, optimized for the MCP ecosystem.

**Tagline:** *Deterministic desktop control for AI agents. Zero vision tokens by default, vision fallback when needed.*

---

## 2. Core Philosophy

| Principle | Description |
|---|---|
| **Accessibility-first** | Use OS-native accessibility trees (AX, UIA, AT-SPI) as the primary interaction model — deterministic, structured, zero vision cost |
| **Vision fallback** | Support screenshot + VLM (vision-language model) for cases where accessibility is unavailable or insufficient |
| **MCP-native** | Expose all capabilities as MCP tools — works with any MCP client (opencode, Claude Code, Cursor, VS Code) |
| **Cross-platform** | First-class support for macOS, Windows, and Linux from day one |
| **Single binary** | Rust compilation produces a single portable executable with no runtime dependencies |
| **Safety** | Sandboxed execution, confirmation gates for destructive actions, network allowlisting |

---

## 3. Feature Set

### 3.1. Desktop Control

| Feature | Priority | Description |
|---|---|---|
| Screen capture (PNG) | P0 | Capture full display or region as PNG for vision analysis |
| Mouse click | P0 | Click at screen coordinates (left, right, middle) |
| Mouse move | P0 | Move cursor to coordinates |
| Type text | P0 | Type a string at current focus |
| Key press | P0 | Press key or key combination (e.g., `Cmd+S`, `Ctrl+C`) |
| Scroll | P0 | Scroll in any direction |
| Drag | P1 | Click-and-drag between coordinates |
| Double/triple click | P1 | Multi-click actions |
| Hold key | P1 | Hold a key for a duration |
| Zoom region | P2 | Request full-resolution capture of a sub-region |
| Wait | P0 | Pause between actions |

### 3.2. Accessibility Tree

| Feature | Priority | Description |
|---|---|---|
| Get tree | P0 | Capture the full accessibility tree as structured JSON |
| Find element | P0 | Locate element by role, label, or text content |
| Get element details | P0 | Get bounds, attributes, state of a specific element |
| Click element | P0 | Click a UI element by accessibility reference |
| Type into element | P0 | Type text into a specific input element |
| Wait for element | P2 | Wait until an element appears or changes state |

### 3.3. Browser Control

| Feature | Priority | Description |
|---|---|---|
| Launch browser | P1 | Open Chrome/Firefox/Safari to a URL |
| Get page DOM | P1 | Extract structured page content via CDP (Chrome) |
| Execute JavaScript | P1 | Run JS in browser context |
| Manage tabs | P2 | List, switch, close tabs |
| Inject script | P2 | Inject user scripts/styles for testing |

### 3.4. System Utilities

| Feature | Priority | Description |
|---|---|---|
| Run command | P0 | Execute a shell command and capture output |
| Get clipboard | P1 | Read current clipboard content |
| Set clipboard | P1 | Write to clipboard |
| Window management | P1 | List, focus, move, resize windows |
| File picker dialog | P2 | Interact with native file open/save dialogs |
| Notification watcher | P2 | Monitor system notifications |

### 3.5. LLM Inference (Local & Cloud)

| Feature | Priority | Description |
|---|---|---|
| Text generation | P1 | General-purpose text inference for planning, reasoning, summarization |
| Vision analysis | P1 | Screenshot analysis via vision-language model |
| Ollama provider | P0 | Local Ollama server (text + vision, zero deps) |
| OpenAI provider | P0 | OpenAI / OpenAI-compatible API (GPT-4o, etc.) |
| Anthropic provider | P0 | Anthropic Messages API (Claude 3.5 Sonnet, etc.) |
| Native llama.cpp | P2 | Optional direct GGUF model loading via `llama-cpp-2` crate (`local-llm` feature) |
| API key from env | P0 | Set `$OPENAI_API_KEY` / `$ANTHROPIC_API_KEY` or reference in config with `$VAR_NAME` |
| Model override | P2 | Per-call model selection via tool arguments |
| Token tracking | P2 | Return token usage in responses |

### 3.6. Safety & Governance

| Feature | Priority | Description |
|---|---|---|
| Confirmation gates | P1 | Require user confirmation for destructive actions |
| Network allowlist | P1 | Restrict which hosts the agent can access |
| Filesystem sandbox | P1 | Restrict file access to approved paths |
| Action audit log | P1 | Record all actions with timestamps |
| Session isolation | P2 | Separate sessions cannot interfere with each other |
| Rate limiting | P2 | Limit actions per second to prevent abuse |

---

## 4. MCP Tool Definitions

All tools follow the MCP specification and are accessible via `stdio` or `SSE` transport.

### 4.1. Core Tools

| Tool Name | Description | Inputs |
|---|---|---|
| `computer/screenshot` | Capture screen or region as base64 PNG | `region?`, `display?` |
| `computer/click` | Mouse click at coordinates | `x`, `y`, `button?`, `clicks?`, `modifiers?` |
| `computer/mouse_move` | Move cursor | `x`, `y` |
| `computer/type` | Type text string | `text` |
| `computer/key` | Press key or combination | `keys` (string or array), `duration?` |
| `computer/scroll` | Scroll at position | `x`, `y`, `delta_x`, `delta_y` |
| `computer/drag` | Click and drag | `start_x`, `start_y`, `end_x`, `end_y` |
| `computer/wait` | Pause execution | `ms` |

### 4.2. Accessibility Tools

| Tool Name | Description | Inputs |
|---|---|---|
| `a11y/tree` | Get accessibility tree | `depth?`, `process?` |
| `a11y/find` | Find element by criteria | `role?`, `label?`, `text?` |
| `a11y/click` | Click element by ref | `ref` (accessibility ID) |
| `a11y/type` | Type into element | `ref`, `text` |
| `a11y/info` | Get element details | `ref` |
| `a11y/info` | Get detailed info about an accessibility element | `ref_id` |
| `a11y/wait` | Poll a11y tree until element appears or timeout | `role?`, `label?`, `text?`, `ref_id?`, `timeout?`, `interval?` |

Auto-falls back to vision analysis (screenshot + VLM) when a11y is unavailable.

### 4.3. Browser Tools

| Tool Name | Description | Inputs |
|---|---|---|
| `browser/open` | Open URL in browser | `url`, `browser?` |
| `browser/dom` | Get page DOM structure | `selector?` |
| `browser/js` | Execute JavaScript | `code` |
| `browser/tabs` | List/open/close tabs | `action`, `tab_id?` |

### 4.4. System Tools

| Tool Name | Description | Inputs |
|---|---|---|
| `system/command` | Run shell command, capture stdout/stderr/exit code | `command`, `timeout?` |
| `system/clipboard` | Read/write clipboard | `action` (read\|write), `text?` |
| `system/windows` | List/focus windows | `action` (list\|focus), `window_id?` |

### 4.5. LLM Tools

| Tool Name | Description | Inputs |
|---|---|---|
| `llm/think` | Text inference for planning/reasoning | `prompt`, `model?` |
| `llm/describe` | Screenshot + vision model analysis | `prompt?`, `model?` |

### 4.6. Vision Tools

| Tool Name | Description | Inputs |
|---|---|---|
| `vision/query` | Analyse screenshot with VLM, return detected UI elements with bounding boxes | `prompt?`, `region?` |

### 4.7. Safety & Meta Tools

| Tool Name | Description | Inputs |
|---|---|---|
| `health/get` | Server health status, version, enabled capabilities | — |
| `audit/recent` | Recent audit log entries | `count?` |

### Enhanced Parameters

| Tool | New Parameter | Description |
|---|---|---|
| `computer/screenshot` | `scale` | HiDPI/Retina scale factor (e.g. 2.0). Multiplies region coords for full-resolution capture |
| `computer/click` | `clicks` | Number of clicks (1=single, 2=double, 3=triple) |
| `computer/key` | `duration` | Hold duration in ms (keydown → wait → keyup) |

### Session Isolation

Each SSE connection gets a unique session UUID. Audit log entries are tagged with `session_id`. Rate limiters are per-session — one session hitting its limit does not affect others. `clone_with_session()` creates isolated `ToolRegistry` instances sharing the same platform backend.

### Test Suite

19 integration tests covering:
- JSON-RPC initialize, tools/list, tools/call, unknown methods, parse errors
- All 26 tool names registered
- Health endpoint, audit/recent logging with session IDs
- Sandbox: command allow/deny/disable, network glob matching, path isolation
- Rate limiter: enabled/disabled, sessions isolated
- Confirmation gate: disabled passthrough
- Session isolation: audit IDs, rate limiter independence

---

## 5. Transport

### 5.1. Stdio (default)
JSON-RPC 2.0 over stdin/stdout. Each line is a JSON-RPC message. Notifications receive no response.

### 5.2. SSE (Server-Sent Events)
HTTP-based transport per the MCP specification:
- `GET /sse` — establishes SSE connection with per-connection UUID session
- `POST /message/{session_id}` — send JSON-RPC to a specific session
- Initial `event: endpoint` with `data: /message/{session_id}` notifies client of its POST URL
- Each session has isolated audit log, rate limiter, and session ID
- Responses delivered via SSE stream and HTTP 200 response

Configure via `--port` flag or `sse { port }` in config file.

---

## 6. Platform Support Matrix

| Capability | macOS | Windows | Linux |
|---|---|---|---|---|
| Accessibility Tree | ✅ osascript/AX | ✅ PowerShell/P-Invoke | ✅ xdotool + wmctrl |
| Screenshot | ✅ screencapture | ✅ PowerShell/.NET | ✅ import/scrot/grim |
| Mouse Input | ✅ osascript | ✅ PowerShell/SendKeys | ✅ xdotool |
| Keyboard Input | ✅ osascript | ✅ PowerShell/SendKeys | ✅ xdotool |
| Clipboard | ✅ pbpaste/pbcopy | ✅ Get/Set-Clipboard | ✅ xclip/xsel/wl-copy |
| Window Mgmt | ✅ osascript | ✅ PowerShell/P-Invoke | ✅ wmctrl |
| Browser CDP | ✅ Chrome/Firefox | ✅ Chrome/Firefox | ✅ Chrome/Firefox |
| Vision Fallback | ✅ VLM (3 providers) | ✅ VLM (3 providers) | ✅ VLM (3 providers) |
| Safety Sandbox | ✅ allowlist + gate | ✅ allowlist + gate | ✅ allowlist + gate |
| Audit Log | ✅ ring buffer | ✅ ring buffer | ✅ ring buffer |
| Rate Limiting | ✅ sliding window | ✅ sliding window | ✅ sliding window |
| SSE Transport | ✅ axum HTTP | ✅ axum HTTP | ✅ axum HTTP |
| Single Binary | ✅ 8.7MB | ✅ 8.7MB | ✅ 8.7MB |
| No Runtime Deps | ✅ (macOS 12+) | ✅ (Windows 10+) | ✅ (xdotool + wmctrl) |

---

## 7. Technical Stack

| Layer | Technology | Rationale |
|---|---|---|
| Language | Rust 2024 edition | Performance, safety, cross-compilation, single binary |
| MCP Protocol | `modelcontextprotocol` Rust SDK | Native MCP support for stdio and SSE transports |
| Accessibility (macOS) | `accessibility` crate + raw `appkit` FFI | Direct AX API bindings |
| Accessibility (Windows) | `windows` crate (UIAutomation) | Official Microsoft Rust projections |
| Accessibility (Linux) | D-Bus via `zbus` crate (AT-SPI2) | Standard Linux accessibility protocol |
| Screenshot (macOS) | CoreGraphics via `core-graphics` crate | Fast, zero-copy screen capture |
| Screenshot (Windows) | `windows` crate (IDXGIOutputDuplication) | DirectX-based capture |
| Screenshot (Linux) | `x11rb` / `wayland` crates | X11: MIT-SHM; Wayland: screencopy |
| Input simulation (macOS) | CoreGraphics via `core-graphics` crate | CGEvent API |
| Input simulation (Windows) | `windows` crate (SendInput) | Win32 input API |
| Input simulation (Linux) | `x11rb` / `libei` / `enigo` | XTest or libei for Wayland |
| Image processing | `image` crate | PNG encode/decode, resize |
| Vision (optional) | HTTP client to VLM server | Ollama, OpenAI, or local vision model |
| Local LLM (Ollama) | HTTP client (`reqwest`) | Zero-dependency local inference via `ollama serve` |
| Local LLM (native) | `llama-cpp-2` crate (optional) | Direct GGUF model loading with `--features local-llm` |
| Logging | `tracing` crate | Structured, async logging |
| Serialization | `serde` + `serde_json` | JSON for MCP messages |

---

## 8. Dependency Graph

```
┌─────────────────────────────────────────────────────────┐
│                    MCP Client                            │
│           (opencode / Claude Code / Cursor)              │
└────────────────────────┬────────────────────────────────┘
                         │ stdio / SSE
                         ▼
┌─────────────────────────────────────────────────────────┐
│              ja-opencode-computer-use                     │
│                                                           │
│  ┌──────────────────────────────────────────────────┐    │
│  │              MCP Server Layer                     │    │
│  │  Tool routing, session management, audit logging │    │
│  └──────────┬──────────┬──────────┬─────────────────┘    │
│             │          │          │                       │
│     ┌───────▼──┐ ┌────▼───┐ ┌───▼────────┐              │
│     │ Desktop  │ │ A11y   │ │ Browser    │              │
│     │ Control  │ │ Tree   │ │ Bridge     │              │
│     └───────┬──┘ └────┬───┘ └───┬────────┘              │
│             │          │          │                       │
│     ┌───────▼──────────▼──────────▼────────┐             │
│     │       Platform Abstraction Layer     │             │
│     │  (trait PlatformBackend)             │             │
│     └───────┬──────────┬──────────┬────────┘             │
│             │          │          │                       │
│     ┌───────▼──┐ ┌────▼───┐ ┌───▼────────┐              │
│     │ macOS    │ │ Windows│ │ Linux      │              │
│     │ (AX, CG) │ │ (UIA)  │ │ (AT-SPI2)  │              │
│     └──────────┘ └────────┘ └────────────┘              │
└─────────────────────────────────────────────────────────┘
```

---

## 9. Development Phases

### Phase 1: Foundation (MVP) ✅
- MCP server with stdio + SSE transport
- Screenshot capture via CLI tools (import/scrot/grim/screencapture/PowerShell)
- Mouse/keyboard input via CLI tools (xdotool/osascript/SendKeys)
- `computer/screenshot`, `computer/click`, `computer/type`, `computer/key`, `computer/mouse_move`
- Cross-platform build system

### Phase 2: Accessibility ✅
- macOS AX via osascript (`screencapture`, `cliclick`)
- Linux accessibility via xdotool + wmctrl
- Windows via PowerShell + Win32 P/Invoke
- `a11y/tree`, `a11y/find`, `a11y/click`, `a11y/type`
- Element reference system
- Auto-fallback to vision when a11y unavailable

### Phase 3: Browser & System ✅
- Chrome DevTools Protocol bridge (WebSocket)
- `browser/open`, `browser/dom`, `browser/js`, `browser/tabs`
- `system/command`, `system/clipboard`, `system/windows`
- Scroll, drag, clipboard, hold-key operations

### Phase 4: Safety & Polish ✅
- SSE transport (axum HTTP server, MCP SSE spec)
- Confirmation gates (configurable, per-tool severity)
- Filesystem sandbox (command allowlisting)
- Network allowlisting (glob patterns)
- Action audit log (ring buffer, `audit/recent` tool)
- Rate limiting (configurable actions/time window)
- `health/get` server status endpoint

### Phase 5: Vision Fallback ✅
- VLM integration (Ollama, OpenAI, Anthropic — all three providers)
- Screenshot-based element detection with bounding box parsing
- Auto-fallback when a11y tools fail (`a11y/*` → vision)
- Standalone `vision/query` tool for direct VLM analysis

---

## 10. Configuration

### Provider examples

**Ollama (local, zero deps):**
```bash
ollama pull llama3.2:3b && ollama pull llama3.2-vision:11b && ollama serve
```

```json
{ "llm": { "enabled": true, "provider": "ollama", "endpoint": "http://localhost:11434" } }
```

**OpenAI (GPT-4o):**
```json
{ "llm": { "enabled": true, "provider": "openai", "text_model": "gpt-4o", "vision_model": "gpt-4o", "api_key": "$OPENAI_API_KEY" } }
```

**Anthropic (Claude 3.5 Sonnet):**
```json
{ "llm": { "enabled": true, "provider": "anthropic", "text_model": "claude-3-5-sonnet-20241022", "vision_model": "claude-3-5-sonnet-20241022", "api_key": "$ANTHROPIC_API_KEY" } }
```

**API keys:** Values prefixed with `$` are read from environment variables. Literal keys are also accepted (not recommended).

**Reusing opencode's LLM:** If opencode sets `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` environment variables when spawning child processes (standard in most MCP hosts), set `api_key` to `"$OPENAI_API_KEY"` and the server will inherit the same credentials. Configure the same model in `text_model` / `vision_model` for consistency.

Configuration via CLI flags, environment variables, and config file (JSON/YAML):

```json
{
  "transport": "stdio",
  "port": 8080,
  "display": 0,
  "sandbox": {
    "enabled": true,
    "allowed_paths": ["/home/user/projects", "/tmp"],
    "allowed_network": ["*.github.com", "api.openai.com"],
    "allowed_commands": ["git", "npm", "cargo", "python"]
  },
  "confirm": {
    "enabled": true
  },
  "audit": {
    "enabled": true,
    "max_entries": 1000
  },
  "rate_limit": {
    "enabled": true,
    "max_actions": 30,
    "window_secs": 1
  },
  "vision": {
    "enabled": false,
    "provider": "ollama",
    "model": "llama3.2-vision",
    "endpoint": "http://localhost:11434",
    "api_key": "$ANTHROPIC_API_KEY"
  },
  "llm": {
    "enabled": false,
    "provider": "ollama",
    "text_model": "llama3.2:3b",
    "vision_model": "llama3.2-vision:11b",
    "endpoint": "http://localhost:11434",
    "api_key": "$OPENAI_API_KEY",
    "model_path": null,
    "n_gpu_layers": 0,
    "max_tokens": 2048,
    "temperature": 0.1
  },
  "browser": {
    "enabled": false,
    "debug_url": "http://localhost:9222",
    "auto_launch": false,
    "browser_path": null
  },
  "logging": {
    "level": "info",
    "file": "/tmp/opencode-computer-use.log"
  }
}
```

---

## 11. Performance Targets

| Metric | Target |
|---|---|
| Screenshot latency | < 50ms |
| Click latency | < 10ms |
| Type latency | < 5ms per character |
| Accessibility tree | < 100ms for full desktop |
| Element find | < 50ms |
| MCP response time | < 5ms (excluding action execution) |
| Binary size | < 7MB stripped |
| Memory usage | < 50MB idle |
