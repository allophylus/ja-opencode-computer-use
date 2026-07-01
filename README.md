# ja-opencode-computer-use

**Desktop control MCP server for AI agents.** Gives opencode, Claude Code, Cursor, and any MCP-compatible client the ability to control desktop applications and web browsers on the host machine.

## Features

- **🖥 Desktop Control** — mouse click, move, drag, scroll, type text, press keys
- **📸 Screen Capture** — capture display as PNG for vision analysis
- **♿ Accessibility Tree** — structured UI tree via OS-native accessibility APIs (deterministic, zero vision cost)
- **🔍 Element Finding** — locate UI elements by role, label, or text
- **🌐 Browser Bridge** — CDP-based Chrome/Firefox control (DOM, JS, tabs)
- **🛡 Safety Sandbox** — confirmation gates, filesystem sandbox, network allowlisting
- **🖼 Vision Fallback** — optional VLM (Ollama/OpenAI) for screenshot-based element detection

## Supported Platforms

| Platform | Tools Used | Status |
|---|---|---|---|
| Linux (X11/Wayland) | `xdotool`, `wmctrl`, `scrot`/`grim`/`import`, `xclip`/`xsel`/`wl-clipboard` | ✅ Tested |
| macOS 12+ | `screencapture`, `osascript`, `pbpaste`, `pbcopy` (all built-in) | 🟡 Designed (untested) |
| Windows 10+ | `powershell` + `System.Drawing`/`System.Windows.Forms` / `user32.dll` P/Invoke | 🟡 Designed (untested) |

## Quick Start

### 1. Install Dependencies

Use the setup script for your platform:

```bash
# Linux — installs xdotool, wmctrl, scrot, xclip, x11-utils
./scripts/setup.sh

# macOS — all tools are built-in (screencapture, osascript, pbcopy, pbpaste)
./scripts/setup.sh

# Windows — all tools are built-in (PowerShell, cmd.exe)
./scripts/setup.sh

# Or install + build the binary + write a default config
./scripts/setup.sh --all
```

### 2. Build

```bash
cargo build --release
cp target/release/ocu ~/.local/bin/
```

### 3. Run

```bash
ocu --help
ocu                                          # stdio transport (default)
ocu --transport sse --port 8080              # SSE transport
ocu --config ~/.config/opencode/ocu.json     # with config file
```

### 4. Add to opencode.json

```json
{
  "mcp": {
    "computer-use": {
      "type": "local",
      "enabled": true,
      "command": "/home/YOU/.local/bin/ocu",
      "args": ["--config", "/home/YOU/.config/opencode/ocu.json"],
      "env": {}
    }
  }
}
```

# Run as SSE server on port 8080
ocu --transport sse --port 8080

# Enable vision fallback
ocu --vision --log-level debug

# Use custom config
ocu --config ~/.config/ocu/config.json
```

## Integration with opencode

Add to your `opencode.json`:

```json
{
  "mcpServers": {
    "computer-use": {
      "command": "ocu",
      "args": ["--transport", "stdio"]
    }
  }
}
```

## MCP Tools

### Computer
| Tool | Description |
|---|---|
| `computer/screenshot` | Capture screen as PNG |
| `computer/click` | Mouse click at coordinates |
| `computer/mouse_move` | Move cursor |
| `computer/type` | Type text |
| `computer/key` | Press key combo |
| `computer/scroll` | Scroll at position |
| `computer/drag` | Click and drag |
| `computer/wait` | Pause execution |

### Accessibility
| Tool | Description |
|---|---|
| `a11y/tree` | Get accessibility tree as JSON |
| `a11y/find` | Find elements by criteria |
| `a11y/click` | Click element by ref |
| `a11y/type` | Type into element by ref |

### System & Browser *(planned)*
| Tool | Description |
|---|---|
| `system/command` | Run shell command |
| `system/clipboard` | Read/write clipboard |
| `system/windows` | List/manage windows |
| `browser/open` | Open URL in browser |
| `browser/dom` | Get page DOM |
| `browser/js` | Execute JavaScript |

## Architecture

```
ocu binary (Rust)
├── MCP Server (rmcp)
├── Tool implementations
├── Platform abstraction traits
└── Platform backends
    ├── macOS (AX, CGEvent, CoreGraphics)
    ├── Windows (UIA, SendInput, DXGI)
    └── Linux (AT-SPI2, XTest, X11/Wayland)
```

## Design Philosophy

1. **Accessibility-first** — uses OS-native a11y APIs for deterministic, token-efficient UI interaction
2. **Vision fallback** — screenshots + VLM for cases where a11y is unavailable
3. **Zero runtime deps** — single statically-linked binary
4. **MCP-native** — works with any MCP client out of the box
5. **Safe by default** — sandbox, confirmation gates, audit log

## License

MIT
