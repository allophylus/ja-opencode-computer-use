use anyhow::{Context, Result};
use crate::platform::{
    A11yNode, AccessibilityTree, Bounds, CommandResult, FindCriteria, InputSimulation, Key,
    MouseButton, Rect, ScreenCapture, SystemTools, WindowInfo,
};

/// Try clipboard via xclip, fall back to xsel, then wl-clipboard (Wayland).
fn clipboard_tool() -> Option<&'static str> {
    for tool in &["xclip", "xsel", "wl-copy"] {
        if std::process::Command::new("which").arg(tool).output().is_ok() {
            return Some(tool);
        }
    }
    None
}

fn tool_to_clipboard_cmd(tool: &str, write: bool) -> Result<(&'static str, &'static [&'static str])> {
    match (tool, write) {
        ("xclip", false) => Ok(("xclip", &["-o", "-selection", "clipboard"])),
        ("xclip", true) => Ok(("xclip", &["-i", "-selection", "clipboard"])),
        ("xsel", false) => Ok(("xsel", &["-o", "-b"])),
        ("xsel", true) => Ok(("xsel", &["-i", "-b"])),
        ("wl-copy", false) => Ok(("wl-paste", &["--"])),
        ("wl-copy", true) => Ok(("wl-copy", &["--"])),
        _ => Err(anyhow::anyhow!("Unknown clipboard tool: {}", tool)),
    }
}

pub struct LinuxBackend;

impl LinuxBackend {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// Capture screen via ImageMagick `import` (X11) or `grim` (Wayland)
    async fn capture_via_tool(region: Option<Rect>) -> Result<Vec<u8>> {
        // Try grim (Wayland first since it's explicit)
        if std::process::Command::new("which").arg("grim").output().is_ok() {
            return Self::capture_via_grim(region).await;
        }
        // Try ImageMagick import (X11)
        if std::process::Command::new("which").arg("import").output().is_ok() {
            return Self::capture_via_import(region).await;
        }
        // Try scrot (X11)
        if std::process::Command::new("which").arg("scrot").output().is_ok() {
            return Self::capture_via_scrot(region).await;
        }
        // Try gnome-screenshot (X11 + Wayland)
        if std::process::Command::new("which").arg("gnome-screenshot").output().is_ok() {
            return Self::capture_via_gnome(region).await;
        }
        Err(anyhow::anyhow!(
            "No screen capture tool found. Install: grim (Wayland), import/imagemagick (X11), scrot, or gnome-screenshot"
        ))
    }

    async fn capture_via_import(region: Option<Rect>) -> Result<Vec<u8>> {
        let mut cmd = tokio::process::Command::new("import");
        cmd.args(["-quiet", "-quality", "95", "png:-"]);
        if let Some(r) = region {
            cmd.arg("-crop").arg(format!("{}x{}+{}+{}", r.width, r.height, r.x, r.y));
        }
        let output = cmd.output().await?;
        if !output.status.success() {
            anyhow::bail!("import failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(output.stdout)
    }

    async fn capture_via_scrot(region: Option<Rect>) -> Result<Vec<u8>> {
        let mut cmd = tokio::process::Command::new("scrot");
        cmd.args(["-q", "95", "-", "--silent"]);
        if let Some(r) = region {
            cmd.arg("-a").arg(format!("{},{},{},{}", r.x, r.y, r.width, r.height));
        }
        let output = cmd.output().await?;
        if !output.status.success() {
            anyhow::bail!("scrot failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(output.stdout)
    }

    async fn capture_via_grim(region: Option<Rect>) -> Result<Vec<u8>> {
        let mut cmd = tokio::process::Command::new("grim");
        cmd.args(["-t", "png", "-"]);
        if let Some(r) = region {
            cmd.arg("-g").arg(format!("{},{} {}x{}", r.x, r.y, r.width, r.height));
        }
        let output = cmd.output().await?;
        if !output.status.success() {
            anyhow::bail!("grim failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(output.stdout)
    }

    async fn capture_via_gnome(region: Option<Rect>) -> Result<Vec<u8>> {
        let file = format!("/tmp/ocu-screenshot-{}.png", std::process::id());
        let mut cmd = tokio::process::Command::new("gnome-screenshot");
        cmd.args(["-f", &file]);
        if region.is_some() {
            cmd.arg("-a"); // area mode (user must click-drag — not great)
        } else {
            cmd.arg("-d").arg("0"); // no delay, full screen
        }
        let output = cmd.output().await?;
        if !output.status.success() {
            anyhow::bail!("gnome-screenshot failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        let png = tokio::fs::read(&file).await?;
        let _ = tokio::fs::remove_file(&file).await;
        Ok(png)
    }
}

// ─── Screen Capture ──────────────────────────────────────────

#[async_trait::async_trait]
impl ScreenCapture for LinuxBackend {
    async fn capture_screen(&self, region: Option<Rect>) -> Result<Vec<u8>> {
        Self::capture_via_tool(region).await
    }

    async fn display_size(&self) -> Result<(u32, u32)> {
        // Try xdpyinfo first
        if let Ok(output) = tokio::process::Command::new("xdpyinfo")
            .args(["-display", ":0"])
            .output().await
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(dims) = line.strip_prefix("dimensions:") {
                    let parts: Vec<&str> = dims.trim().splitn(2, 'x').collect();
                    if parts.len() >= 2 {
                        let w: u32 = parts[0].trim().parse().unwrap_or(1920);
                        let h: u32 = parts[1].trim().split_whitespace().next().unwrap_or("1080").parse().unwrap_or(1080);
                        return Ok((w, h));
                    }
                }
            }
        }
        // Try xdotool
        if let Ok(output) = tokio::process::Command::new("xdotool")
            .args(["getdisplaygeometry"])
            .output().await
        {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let parts: Vec<&str> = stdout.trim().split_whitespace().collect();
            if parts.len() >= 2 {
                return Ok((parts[0].parse()?, parts[1].parse()?));
            }
        }
        Ok((1920, 1080))
    }
}

// ─── Input Simulation ────────────────────────────────────────

/// Run xdotool and check success
async fn xdotool(args: &[&str]) -> Result<String> {
    let output = tokio::process::Command::new("xdotool")
        .args(args).output().await
        .map_err(|e| anyhow::anyhow!("xdotool not available: {}", e))?;
    if !output.status.success() {
        anyhow::bail!("xdotool {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn xdotool_button(b: MouseButton) -> &'static str {
    match b { MouseButton::Left => "1", MouseButton::Middle => "2", MouseButton::Right => "3" }
}

fn xdotool_key(key: &Key) -> String {
    match key {
        Key::Char(c) => c.to_string(),
        Key::Named(n) => {
            let s = match n.to_lowercase().as_str() {
                "enter" | "return" => "Return", "tab" => "Tab",
                "escape" | "esc" => "Escape", "space" => "space",
                "backspace" => "BackSpace", "delete" => "Delete",
                "shift" => "Shift_L", "control" | "ctrl" => "Control_L",
                "alt" => "Alt_L", "meta" | "cmd" | "super" | "win" => "Super_L",
                "capslock" | "caps_lock" => "Caps_Lock",
                "up" => "Up", "down" => "Down", "left" => "Left", "right" => "Right",
                "home" => "Home", "end" => "End",
                "pageup" | "page_up" => "Page_Up", "pagedown" | "page_down" => "Page_Down",
                "f1" => "F1", "f2" => "F2", "f3" => "F3", "f4" => "F4",
                "f5" => "F5", "f6" => "F6", "f7" => "F7", "f8" => "F8",
                "f9" => "F9", "f10" => "F10", "f11" => "F11", "f12" => "F12",
                _ => n,
            };
            s.to_string()
        }
    }
}

#[async_trait::async_trait]
impl InputSimulation for LinuxBackend {
    async fn mouse_move(&self, x: i32, y: i32) -> Result<()> {
        xdotool(&["mousemove", &x.to_string(), &y.to_string()]).await?;
        Ok(())
    }

    async fn mouse_click(&self, x: i32, y: i32, button: MouseButton, clicks: u32) -> Result<()> {
        xdotool(&["mousemove", &x.to_string(), &y.to_string()]).await?;
        let btn = xdotool_button(button);
        for _ in 0..clicks {
            xdotool(&["click", btn]).await?;
        }
        Ok(())
    }

    async fn mouse_drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<()> {
        xdotool(&["mousemove", &from.0.to_string(), &from.1.to_string()]).await?;
        xdotool(&["mousedown", "1"]).await?;
        xdotool(&["mousemove", &to.0.to_string(), &to.1.to_string()]).await?;
        xdotool(&["mouseup", "1"]).await?;
        Ok(())
    }

    async fn mouse_scroll(&self, x: i32, y: i32, _dx: i32, dy: i32) -> Result<()> {
        xdotool(&["mousemove", &x.to_string(), &y.to_string()]).await?;
        let dir = if dy > 0 { 4 } else { 5 }; // 4=scroll up, 5=scroll down (button clicks)
        let count = dy.unsigned_abs();
        for _ in 0..count {
            xdotool(&["click", &dir.to_string()]).await?;
        }
        Ok(())
    }

    async fn type_text(&self, text: &str) -> Result<()> {
        // Use type --delay 0 for speed; quote the text
        xdotool(&["type", "--delay", "0", text]).await?;
        Ok(())
    }

    async fn key_press(&self, keys: &[Key], duration_ms: Option<u64>) -> Result<()> {
        let xd_keys: Vec<String> = keys.iter().map(xdotool_key).collect();
        let joined: Vec<&str> = xd_keys.iter().map(|s| s.as_str()).collect();

        if let Some(ms) = duration_ms {
            // Hold: keydown, wait, keyup
            for k in &joined {
                xdotool(&["keydown", k]).await?;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
            for k in joined.iter().rev() {
                xdotool(&["keyup", k]).await?;
            }
        } else {
            xdotool(&[&["key"] as &[_], &joined].concat()).await?;
        }
        Ok(())
    }
}

// ─── Accessibility Tree ──────────────────────────────────────

#[async_trait::async_trait]
impl AccessibilityTree for LinuxBackend {
    async fn get_tree(&self, _depth: Option<u32>) -> Result<A11yNode> {
        let output = tokio::process::Command::new("xdotool")
            .args(["getactivewindow"]).output().await;
        let win_id = match output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim().to_string(),
            Err(_) => "root".into(),
        };

        let title = if win_id != "root" {
            tokio::process::Command::new("xdotool")
                .args(["getwindowname", &win_id]).output().await
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default()
        } else {
            "Linux Desktop".into()
        };

        let (w, h) = self.display_size().await.unwrap_or((1920, 1080));

        Ok(A11yNode {
            ref_id: win_id,
            role: "window".into(),
            label: title,
            bounds: Bounds { x: 0, y: 0, w, h },
            enabled: true, focused: true,
            children: vec![],
            actions: vec!["screenshot".into(), "click".into(), "type".into()],
        })
    }

    async fn find_element(&self, criteria: &FindCriteria) -> Result<Vec<A11yNode>> {
        let mut args: Vec<String> = vec!["search".into()];
        if let Some(label) = &criteria.label { args.push("--name".into()); args.push(label.clone()); }
        if let Some(role) = &criteria.role { args.push("--class".into()); args.push(role.clone()); }
        if let Some(text) = &criteria.text { args.push("--name".into()); args.push(text.clone()); }

        let output = tokio::process::Command::new("xdotool")
            .args(&args).output().await
            .map_err(|e| anyhow::anyhow!("xdotool search: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let ids: Vec<&str> = stdout.lines().collect();
        let mut results = Vec::new();

        for id in ids {
            let title = tokio::process::Command::new("xdotool")
                .args(["getwindowname", id]).output().await
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();

            let geom = tokio::process::Command::new("xdotool")
                .args(["getwindowgeometry", id]).output().await;
            let bounds = match geom {
                Ok(g) => {
                    let s = String::from_utf8_lossy(&g.stdout);
                    let mut bx = 0i32; let mut by = 0i32; let mut bw = 0u32; let mut bh = 0u32;
                    for line in s.lines() {
                        if let Some(x) = line.strip_prefix("  Position: ") {
                            let coords: Vec<i32> = x.split(',').filter_map(|v| v.trim().parse().ok()).collect();
                            if coords.len() >= 2 { bx = coords[0]; by = coords[1]; }
                        }
                        if let Some(x) = line.strip_prefix("  Geometry: ") {
                            let dims: Vec<u32> = x.split('x').filter_map(|v| v.trim().parse().ok()).collect();
                            if dims.len() >= 2 { bw = dims[0]; bh = dims[1]; }
                        }
                    }
                    Bounds { x: bx, y: by, w: bw, h: bh }
                }
                Err(_) => Bounds { x: 0, y: 0, w: 0, h: 0 },
            };

            results.push(A11yNode {
                ref_id: id.to_string(),
                role: "window".into(),
                label: title,
                bounds,
                enabled: true, focused: false,
                children: vec![],
                actions: vec![],
            });
        }
        Ok(results)
    }

    async fn get_element_info(&self, ref_id: &str) -> Result<A11yNode> {
        let nodes = self.find_element(&FindCriteria {
            role: None,
            label: None,
            text: Some(ref_id.to_string()),
        }).await?;
        nodes.into_iter().next().context("Element not found")
    }

    async fn click_element(&self, ref_id: &str) -> Result<()> {
        tokio::process::Command::new("xdotool")
            .args(["windowactivate", "--sync", ref_id]).output().await
            .map_err(|e| anyhow::anyhow!("xdotool focus: {}", e))?;
        tokio::process::Command::new("xdotool")
            .args(["click", "--window", ref_id, "1"]).output().await
            .map_err(|e| anyhow::anyhow!("xdotool click: {}", e))?;
        Ok(())
    }

    async fn type_into_element(&self, ref_id: &str, text: &str) -> Result<()> {
        tokio::process::Command::new("xdotool")
            .args(["windowactivate", "--sync", ref_id]).output().await
            .map_err(|e| anyhow::anyhow!("xdotool focus: {}", e))?;
        tokio::process::Command::new("xdotool")
            .args(["type", "--window", ref_id, text]).output().await
            .map_err(|e| anyhow::anyhow!("xdotool type: {}", e))?;
        Ok(())
    }
}

// ─── System Tools ────────────────────────────────────────────

#[async_trait::async_trait]
impl SystemTools for LinuxBackend {
    async fn run_command(&self, command: &str, timeout_secs: u64) -> Result<CommandResult> {
        let output = tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            tokio::process::Command::new("sh").args(["-c", command]).output(),
        ).await.map_err(|_| anyhow::anyhow!("Command timed out after {}s", timeout_secs))??;

        Ok(CommandResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    async fn clipboard_get(&self) -> Result<String> {
        let tool = clipboard_tool().context("Install xclip, xsel, or wl-clipboard")?;
        let (cmd, args) = tool_to_clipboard_cmd(tool, false)?;
        let output = tokio::process::Command::new(cmd).args(args).output().await?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    async fn clipboard_set(&self, text: &str) -> Result<()> {
        let tool = clipboard_tool().context("Install xclip, xsel, or wl-clipboard")?;
        let (cmd, args) = tool_to_clipboard_cmd(tool, true)?;
        let mut child = tokio::process::Command::new(cmd).args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()?;
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(text.as_bytes()).await?;
        }
        child.wait().await?;
        Ok(())
    }

    async fn list_windows(&self) -> Result<Vec<WindowInfo>> {
        let output = tokio::process::Command::new("wmctrl").args(["-l"]).output().await
            .map_err(|e| anyhow::anyhow!("wmctrl not available: {}", e))?;

        let mut windows = Vec::new();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let parts: Vec<&str> = line.splitn(4, char::is_whitespace).filter(|s| !s.is_empty()).collect();
            if parts.len() >= 4 {
                windows.push(WindowInfo {
                    id: parts[0].to_string(),
                    title: parts[3].to_string(),
                    app_name: parts[2].to_string(),
                    bounds: Bounds { x: 0, y: 0, w: 0, h: 0 },
                    minimized: false, focused: false,
                });
            }
        }
        Ok(windows)
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        tokio::process::Command::new("wmctrl").args(["-i", "-a", window_id]).output().await
            .map_err(|e| anyhow::anyhow!("wmctrl focus: {}", e))?;
        Ok(())
    }
}
