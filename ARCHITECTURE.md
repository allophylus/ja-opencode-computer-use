# Architecture — ja-opencode-computer-use

## Design Overview

```
┌──────────────────────────────────────────────────────────────┐
│                        MCP Client                            │
│               (opencode / Claude Code / Cursor)              │
└────────────────────┬─────────────────────────────────────────┘
                     │ JSON-RPC over stdio
                     ▼
┌──────────────────────────────────────────────────────────────┐
│                     ocu (Binary)                              │
│                                                               │
│  ┌──────────────────────────────────────────────────────────┐│
│  │  MCP Server Layer (rmcp)                                  ││
│  │  ┌─────────────┐ ┌─────────────┐ ┌────────────────────┐  ││
│  │  │ Tool Router  │ │ Session Mgr │ │ Confirmation Gate  │  ││
│  │  └──────┬──────┘ └─────────────┘ └────────────────────┘  ││
│  └─────────┼────────────────────────────────────────────────┘│
│            │                                                  │
│  ┌─────────▼────────────────────────────────────────────────┐│
│  │  Tool Implementations                                    ││
│  │  ┌──────────┐ ┌─────────┐ ┌────────┐ ┌───────────────┐  ││
│  │  │Computer  │ │ A11y    │ │Browser │ │ System        │  ││
│  │  │(screenshot│ │ (tree,  │ │ (CDP,  │ │ (command,     │  ││
│  │  │, click,  │ │ find,   │ │ DOM,   │ │ clipboard,    │  ││
│  │  │ type,key)│ │ click)  │ │ JS)    │ │ windows)      │  ││
│  │  └────┬─────┘ └───┬─────┘ └───┬────┘ └───────┬───────┘  ││
│  └───────┼───────────┼───────────┼───────────────┼──────────┘│
│          │           │           │               │           │
│  ┌───────▼───────────▼───────────▼───────────────▼──────────┐│
│  │  Platform Abstraction (trait PlatformBackend)            ││
│  │  ┌─────────────────┐ ┌──────────────┐ ┌───────────────┐  ││
│  │  │ DesktopOps      │ │ A11yOps      │ │ ScreenCapture │  ││
│  │  │ (mouse,keyboard)│ │ (tree, find) │ │ (screenshot)  │  ││
│  │  └─────────────────┘ └──────────────┘ └───────────────┘  ││
│  └─────────────────────┬────────────────────────────────────┘│
│                        │                                      │
│  ┌─────────────────────▼────────────────────────────────────┐│
│  │  Platform Backends                                       ││
│  │  ┌──────────────┐ ┌──────────┐ ┌──────────────────────┐  ││
│  │  │ macOS        │ │ Windows    │ │ Linux                │  ││
│  │  │ • osascript   │ │ • PowerShell│ │ • xdotool             │  ││
│  │  │ • screencapture│ │ • .NET Win32│ │ • wmctrl              │  ││
│  │  │ • pbpaste/pbcopy│ │ (built-in) │ │ • import/scrot/grim  │  ││
│  │  │ (built-in CLI) │ │            │ │ • xclip/wl-clipboard │  ││
│  │  └──────────────┘ └──────────┘ └──────────────────────┘  ││
│  └──────────────────────────────────────────────────────────┘│
└──────────────────────────────────────────────────────────────┘
```

## Backend Trait Design

All platform-specific operations are defined by Rust traits. Platform backends implement these traits; the tool layer calls them generically.

### Core Traits

```rust
/// Screen capture operations
#[async_trait]
trait ScreenCapture: Send + Sync {
    /// Capture the full display or a region as PNG bytes
    async fn capture_screen(&self, region: Option<Rect>) -> Result<Vec<u8>>;
    /// Get display dimensions
    async fn display_size(&self) -> Result<(u32, u32)>;
}

/// Mouse and keyboard input
#[async_trait]
trait InputSimulation: Send + Sync {
    async fn mouse_move(&self, x: i32, y: i32) -> Result<()>;
    async fn mouse_click(&self, x: i32, y: i32, button: MouseButton, clicks: u32) -> Result<()>;
    async fn mouse_drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<()>;
    async fn mouse_scroll(&self, x: i32, y: i32, delta_x: i32, delta_y: i32) -> Result<()>;
    async fn type_text(&self, text: &str) -> Result<()>;
    async fn key_press(&self, keys: &[Key], duration_ms: Option<u64>) -> Result<()>;
}

/// Accessibility tree operations
#[async_trait]
trait AccessibilityTree: Send + Sync {
    async fn get_tree(&self, depth: Option<u32>) -> Result<A11yNode>;
    async fn find_element(&self, criteria: &FindCriteria) -> Result<Vec<A11yNode>>;
    async fn get_element_info(&self, ref_id: &str) -> Result<A11yNode>;
    async fn click_element(&self, ref_id: &str) -> Result<()>;
    async fn type_into_element(&self, ref_id: &str, text: &str) -> Result<()>;
}

/// Complete platform backend
#[async_trait]
trait PlatformBackend: ScreenCapture + InputSimulation + AccessibilityTree {}
```

### Platform Selection

Platform backend is selected at compile time via `#[cfg]` and at runtime via an enum:

```rust
enum Platform {
    macOS(MacOSBackend),
    Windows(WindowsBackend),
    Linux(LinuxBackend),
}

impl Platform {
    fn new() -> Result<Self> {
        #[cfg(target_os = "macos")]
        { Ok(Self::macOS(MacOSBackend::new()?)) }
        #[cfg(target_os = "windows")]
        { Ok(Self::Windows(WindowsBackend::new()?)) }
        #[cfg(target_os = "linux")]
        { Ok(Self::Linux(LinuxBackend::new()?)) }
    }
}

#[async_trait]
impl PlatformBackend for Platform {
    // Delegates to the inner backend
    async fn capture_screen(&self, region: Option<Rect>) -> Result<Vec<u8>> {
        match self {
            Self::macOS(b) => b.capture_screen(region).await,
            Self::Windows(b) => b.capture_screen(region).await,
            Self::Linux(b) => b.capture_screen(region).await,
        }
    }
    // ... etc
}
```

## MCP Tool Definitions

Tools are defined using the `rmcp` crate's tool macro system:

```rust
#[tool(
    name = "computer/screenshot",
    description = "Capture the current screen as a base64-encoded PNG image"
)]
async fn screenshot(
    region: Option<RectParam>,
    display: Option<u32>,
) -> Result<ImageContent, ToolError> {
    let png_bytes = platform.capture_screen(region.map(Into::into)).await?;
    let b64 = base64::Engine::encode(&png_bytes);
    Ok(ImageContent { data: b64, mime_type: "image/png".into() })
}
```

## Accessibility Tree Structure

The accessibility tree is returned as a JSON structure designed for LLM consumption:

```json
{
  "ref": "root_1",
  "role": "application",
  "label": "Finder",
  "bounds": { "x": 0, "y": 0, "w": 1920, "h": 1080 },
  "children": [
    {
      "ref": "menu_2",
      "role": "menu bar",
      "label": "",
      "bounds": { "x": 0, "y": 0, "w": 1920, "h": 24 },
      "children": [
        {
          "ref": "menu_item_3",
          "role": "menu item",
          "label": "File",
          "bounds": { "x": 0, "y": 0, "w": 40, "h": 24 },
          "enabled": true,
          "focused": false,
          "children": [],
          "actions": ["press"]
        }
      ]
    }
  ]
}
```

The tree uses **progressive skeleton traversal** to keep token usage efficient:
- Top-level: role, label, bounds, ref only
- Children are included recursively
- Each node includes an `actions` array showing what interactions are available

## Safety Architecture

```
┌──────────┐    ┌──────────────┐    ┌──────────────┐
│  Tool    │───►│  Permission  │───►│  Action      │
│  Request │    │  Gate        │    │  Executor    │
└──────────┘    └──────────────┘    └──────────────┘
                      │                    │
                      ▼                    ▼
              ┌──────────────┐    ┌──────────────┐
              │  Config      │    │  Audit Log   │
              │  Rules       │    │  (JSON)      │
              └──────────────┘    └──────────────┘
```

**Permission levels:**
1. **Allow** — always permitted (screenshot, move mouse, get tree)
2. **Confirm** — requires user confirmation (click, type, key press, command)
3. **Block** — never permitted (configurable)

## Browser Control Bridge

Browser control uses the Chrome DevTools Protocol (CDP) via a WebSocket connection:

```
┌──────────┐     CDP WebSocket     ┌──────────┐
│  Agent   │◄────────────────────►│  Chrome  │
│  (CDP)   │                      │  Browser │
└──────────┘                      └──────────┘
```

- Launch Chrome with `--remote-debugging-port=9222`
- Connect via WebSocket to `ws://127.0.0.1:9222/devtools/browser/<id>`
- Use `Runtime.evaluate` for JS, `DOM.getDocument` for page structure, `Page.captureScreenshot` for page screenshots

## Configuration Loading

Config is loaded with the following priority (highest wins):
1. CLI flags
2. Environment variables (prefix `OCU_`)
3. Config file (`~/.config/ocu/config.json`)
4. Built-in defaults
