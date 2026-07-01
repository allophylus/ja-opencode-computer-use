#!/usr/bin/env bash
set -euo pipefail

# ─── ocu — cross-platform computer-use MCP server installer ────────────────
# Detects the OS and installs the required CLI dependencies for the platform.
# Optionally builds the Rust binary and places it at ~/.local/bin/ocu.
#
# Usage:
#   ./scripts/setup.sh                    # install deps only
#   ./scripts/setup.sh --build            # install deps + compile binary
#   ./scripts/setup.sh --config           # install deps + write base config
#   ./scripts/setup.sh --all              # deps + build + config + opencode
# ────────────────────────────────────────────────────────────────────────────

BUILD="${BUILD:-false}"
CONFIG="${CONFIG:-false}"

for arg in "$@"; do
  case "$arg" in
    --build|--binary) BUILD=true ;;
    --config)         CONFIG=true ;;
    --all)            BUILD=true; CONFIG=true ;;
  esac
done

OS="$(uname -s)"
ARCH="$(uname -m)"
PREFIX="${PREFIX:-$HOME/.local}"

info()  { printf "\033[36m==>\033[0m %s\n" "$*"; }
ok()    { printf "\033[32m  OK\033[0m  %s\n" "$*"; }
warn()  { printf "\033[33m WARN\033[0m  %s\n" "$*" >&2; }
err()   { printf "\033[31mFAIL\033[0m  %s\n" "$*" >&2; exit 1; }

needs_cmd() {
  if ! command -v "$1" &>/dev/null; then
    err "Required command not found: $1. Install it and re-run."
  fi
}

# ─── Detect package manager ──────────────────────────────────────────────────
detect_pkg_manager() {
  if command -v apt-get &>/dev/null; then echo "apt"
  elif command -v dnf &>/dev/null; then echo "dnf"
  elif command -v yum &>/dev/null; then echo "yum"
  elif command -v pacman &>/dev/null; then echo "pacman"
  elif command -v zypper &>/dev/null; then echo "zypper"
  elif command -v brew &>/dev/null; then echo "brew"
  else echo "unknown"
  fi
}

install_pkg() {
  local pkg="$1"
  case "$PM" in
    apt)    sudo apt-get install -y -qq "$pkg" ;;
    dnf|yum) sudo "$PM" install -y "$pkg" ;;
    pacman) sudo pacman -S --noconfirm "$pkg" ;;
    zypper) sudo zypper --non-interactive install "$pkg" ;;
    brew)   brew install "$pkg" ;;
    *)      warn "No package manager found — please install $pkg manually" ;;
  esac
}

# ═══════════════════════════════════════════════════════════════════════════
#  LINUX
# ═══════════════════════════════════════════════════════════════════════════
setup_linux() {
  info "Detected Linux ($ARCH)"

  PM="$(detect_pkg_manager)"
  info "Package manager: $PM"

  # Core input simulation & window management
  install_pkg "xdotool"
  install_pkg "wmctrl"

  # Screen capture (pick one)
  if ! command -v import &>/dev/null && ! command -v scrot &>/dev/null \
     && ! command -v grim &>/dev/null && ! command -v gnome-screenshot &>/dev/null; then
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
      install_pkg "grim"
      install_pkg "slurp"
    else
      install_pkg "scrot"
    fi
  fi

  # Clipboard tools (pick one)
  if ! command -v xclip &>/dev/null && ! command -v xsel &>/dev/null \
     && ! command -v wl-copy &>/dev/null; then
    if [ -n "${WAYLAND_DISPLAY:-}" ]; then
      install_pkg "wl-clipboard"
    else
      install_pkg "xclip"
    fi
  fi

  # Display info
  if ! command -v xdpyinfo &>/dev/null; then
    install_pkg "x11-utils"
  fi

  needs_cmd "curl"

  ok "All Linux dependencies installed"
}

# ═══════════════════════════════════════════════════════════════════════════
#  macOS
# ═══════════════════════════════════════════════════════════════════════════
setup_macos() {
  info "Detected macOS ($ARCH)"

  if ! command -v osascript &>/dev/null; then
    err "osascript not found — this doesn't look like a standard macOS install"
  fi

  ok "All macOS dependencies are built-in (no install needed)"

  warn "┌─────────────────────────────────────────────────────────────────┐"
  warn "│  macOS system permissions required                             │"
  warn "│                                                                 │"
  warn "│  1. Accessibility: System Settings → Privacy & Security →       │"
  warn "│     Accessibility → add your terminal app                       │"
  warn "│  2. Screen Recording: System Settings → Privacy & Security →    │"
  warn "│     Screen Recording → add your terminal app                    │"
  warn "└─────────────────────────────────────────────────────────────────┘"
}

# ═══════════════════════════════════════════════════════════════════════════
#  Windows (MSYS2/MinGW/Git Bash / WSL)
# ═══════════════════════════════════════════════════════════════════════════
setup_windows() {
  info "Detected Windows ($ARCH)"

  if ! command -v powershell &>/dev/null; then
    warn "powershell not found in PATH — ocu may not work in this shell"
  fi

  ok "All Windows dependencies are built-in (PowerShell + .NET)"
}

# ═══════════════════════════════════════════════════════════════════════════
#  BUILD
# ═══════════════════════════════════════════════════════════════════════════
build_binary() {
  info "Building ocu binary (release)"

  needs_cmd "cargo"

  local src
  src="$(cd "$(dirname "$0")/.." && pwd)"
  cd "$src"

  cargo build --release 2>&1
  mkdir -p "$PREFIX/bin"
  cp target/release/ocu "$PREFIX/bin/ocu"
  ok "Binary placed at $PREFIX/bin/ocu"
}

# ═══════════════════════════════════════════════════════════════════════════
#  CONFIG
# ═══════════════════════════════════════════════════════════════════════════
write_config() {
  info "Writing default config to $PREFIX/share/ocu/config.json"

  mkdir -p "$PREFIX/share/ocu"

  cat > "$PREFIX/share/ocu/config.json" << 'EOF'
{
  "transport": "stdio",
  "display": { "number": 0 },
  "sandbox": {
    "enabled": true,
    "allowed_paths": ["/home/USER"],
    "allowed_network": [],
    "allowed_commands": ["git", "ls", "cat", "pwd", "which", "echo", "mkdir", "cp", "mv", "rm", "touch", "chmod"]
  },
  "vision": {
    "enabled": false,
    "provider": "ollama",
    "model": "llama3.2-vision",
    "endpoint": "http://localhost:11434",
    "api_key": null
  },
  "llm": {
    "enabled": false,
    "provider": "ollama",
    "text_model": "llama3.2:3b",
    "vision_model": "llama3.2-vision:11b",
    "endpoint": "http://localhost:11434",
    "api_key": null,
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
    "file": null
  },
  "confirm": { "enabled": true },
  "audit": { "enabled": true, "max_entries": 1000 },
  "rate_limit": { "enabled": true, "max_actions": 30, "window_secs": 1 }
}
EOF

  local user="${USER:-$(whoami)}"
  sed -i "s|/home/USER|/home/$user|g" "$PREFIX/share/ocu/config.json"

  ok "Config written to $PREFIX/share/ocu/config.json"
}

# ═══════════════════════════════════════════════════════════════════════════
#  OPENCODE INTEGRATION
# ═══════════════════════════════════════════════════════════════════════════
# Auto-detect opencode config (XDG) and add/update computer-use MCP entry.
# Uses jq when available; falls back to printing the snippet.
# ────────────────────────────────────────────────────────────────────────────
setup_opencode_integration() {
  if ! command -v jq &>/dev/null; then
    warn "jq not found — skipping opencode.json auto-integration."
    info "Manually add this to your opencode config:"
    echo ""
    echo '    "computer-use": {'
    echo '      "type": "local",'
    echo '      "enabled": true,'
    echo '      "command": "'"$PREFIX"'/bin/ocu",'
    echo '      "args": ["--config", "'"$PREFIX"'/share/ocu/config.json"],'
    echo '      "env": {}'
    echo '    }'
    echo ""
    return
  fi

  local config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
  local opencode_dir="$config_home/opencode"

  # Find the first existing opencode config file (in loading order)
  local target=""
  for f in "$opencode_dir/opencode.json" "$opencode_dir/config.json"; do
    if [ -f "$f" ]; then
      target="$f"
      break
    fi
  done

  # If none exists, create opencode.json
  if [ -z "$target" ]; then
    mkdir -p "$opencode_dir"
    target="$opencode_dir/opencode.json"
    echo '{}' > "$target"
  fi

  # Build the MCP entry with the user's PREFIX
  local mcp_entry
  mcp_entry=$(jq -n \
    --arg cmd "$PREFIX/bin/ocu" \
    --arg cfg "$PREFIX/share/ocu/config.json" \
    '{
      "type": "local",
      "enabled": true,
      "command": $cmd,
      "args": ["--config", $cfg],
      "env": {}
    }')

  # Merge into the target config (creates .mcp if missing)
  local merged
  merged=$(jq --argjson entry "$mcp_entry" \
    '.mcp = ((.mcp // {}) | .["computer-use"] = $entry)' \
    "$target") && echo "$merged" > "$target"

  if [ $? -eq 0 ]; then
    ok "OpenCode MCP entry added to $target"
  else
    warn "Failed to update opencode config at $target"
    info "Add this entry manually:"
    echo "$mcp_entry"
  fi
}

# ═══════════════════════════════════════════════════════════════════════════
#  MAIN
# ═══════════════════════════════════════════════════════════════════════════
main() {
  case "$OS" in
    Linux)  setup_linux  ;;
    Darwin) setup_macos  ;;
    MINGW*|MSYS*|CYGWIN*) setup_windows ;;
    *)      err "Unsupported OS: $OS" ;;
  esac

  if [ "$BUILD" = true ]; then
    build_binary
  fi

  if [ "$CONFIG" = true ]; then
    write_config
  fi

  echo ""
  info "Done. Run 'ocu --help' to verify installation."
  setup_opencode_integration
}

main
