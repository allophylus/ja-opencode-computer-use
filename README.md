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

## Platform Support & Requirements

| Platform | Tools Used | Status |
|---|---|---|---|
| Linux (X11/Wayland) | `xdotool`, `wmctrl`, `scrot`/`grim`/`import`, `xclip`/`xsel`/`wl-clipboard` | ✅ Tested |
| macOS 12+ | `osascript`, `screencapture`, `pbpaste`, `pbcopy` (all built-in) | 🟡 Tested (community) |
| Windows 10+ | `powershell`, `cmd.exe`, .NET Framework (built-in) | 🟡 Designed (untested) |

## Permissions & Setup by OS

### macOS

macOS uses **built-in CLI tools** — no package install needed. However, **two system permissions** must be granted:

| Permission | Required For | How to Grant |
|---|---|---|
| **Accessibility** | Input simulation (click, type, key press, scroll, drag), window listing, accessibility tree | System Settings → Privacy & Security → Accessibility → add your terminal app |
| **Screen Recording** | Screen capture (`screencapture`) | System Settings → Privacy & Security → Screen Recording → add your terminal app |

> **Important:** Grant these permissions to the app that runs the `ocu` binary (e.g. Terminal, iTerm2, Warp, VS Code). If `ocu` runs as an MCP subprocess, you may need to grant permissions to the host app (e.g. Terminal if you launched opencode from it).

**Verification:**
```bash
# Check if osascript can list windows (requires Accessibility)
osascript -e 'tell app "System Events" to get name of every window of every process'

# Check if screencapture works (requires Screen Recording)
screencapture -x /tmp/test-capture.png && open /tmp/test-capture.png
```

**Setup:**
```bash
# All tools are built-in — no packages to install
./scripts/setup.sh
```

### Linux (X11)

Linux uses external CLI tools that must be installed via your package manager:

| Tool | Required For | Package Name |
|---|---|---|
| `xdotool` | Mouse, keyboard, window search, geometry | `xdotool` |
| `wmctrl` | Window listing and focus | `wmctrl` |
| `scrot` *or* `import` (ImageMagick) | Screen capture (X11) | `scrot` or `imagemagick` |
| `xclip` *or* `xsel` | Clipboard read/write (X11) | `xclip` or `xsel` |
| `xdpyinfo` (from `x11-utils`) | Display dimensions | `x11-utils` |

**Wayland alternatives:**

| Tool | Required For | Package Name |
|---|---|---|
| `grim` + `slurp` | Screen capture | `grim`, `slurp` |
| `wl-clipboard` | Clipboard read/write | `wl-clipboard` |

**Permissions:**
- No OS-level permission grants needed
- `xdotool` needs access to the X11 display (`$DISPLAY` must be set, typically `:0`)
- On Wayland, `xdotool`/`wmctrl` may not work — use the Wayland alternatives above

**Setup:**
```bash
./scripts/setup.sh            # auto-detects display server and installs appropriate tools
```

### Windows

Windows uses **built-in tools** — no package install needed.

| Tool | Required For | Availability |
|---|---|---|
| `powershell.exe` | All operations (mouse, keyboard, screenshot, clipboard, windows) | Built-in (Windows 10+) |
| `cmd.exe` | Shell command execution | Built-in |
| .NET Framework | `System.Drawing`, `System.Windows.Forms` for screenshot/mouse/keyboard | Built-in (Windows 10+) |

**Permissions:**
- No special OS-level permissions required for default operation
- PowerShell execution policy must allow script execution (typically already set)
- Some operations (e.g. window enumeration via `user32.dll` P/Invoke) are always allowed

**Setup:**
```bash
./scripts/setup.sh            # verifies PowerShell availability
```

## Quick Start

### 1. Install Dependencies & Permissions

Follow the per-OS guide above to install tools (Linux) or grant permissions (macOS), then:

```bash
./scripts/setup.sh
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

### 4. Add to opencode.json (opencode)

```json
{
  "mcpServers": {
    "computer-use": {
      "command": "/home/YOU/.local/bin/ocu",
      "args": ["--config", "/home/YOU/.config/opencode/ocu.json"]
    }
  }
}
```

### 5. Add to opencode.json (Claude Code / Cursor / other MCP clients)

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
