use anyhow::{Context, Result};
use crate::platform::{
    A11yNode, AccessibilityTree, Bounds, CommandResult, FindCriteria, InputSimulation, Key,
    MouseButton, Rect, ScreenCapture, SystemTools, WindowInfo,
};

pub struct MacOSBackend;

impl MacOSBackend {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// Helper: run a shell command and return stdout.
    async fn run(cmd: &str, args: &[&str]) -> Result<String> {
        let output = tokio::process::Command::new(cmd).args(args)
            .output().await
            .with_context(|| format!("Failed to run: {} {:?}", cmd, args))?;
        if !output.status.success() {
            anyhow::bail!("{} {:?} failed: {}", cmd, args, String::from_utf8_lossy(&output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Helper: run osascript.
    async fn osascript(script: &str) -> Result<String> {
        Self::run("osascript", &["-e", script]).await
    }
}

// ─── Screen Capture ──────────────────────────────────────────

#[async_trait::async_trait]
impl ScreenCapture for MacOSBackend {
    async fn capture_screen(&self, region: Option<Rect>) -> Result<Vec<u8>> {
        let file = format!("/tmp/ocu-screenshot-{}.png", std::process::id());
        if let Some(r) = region {
            Self::run("screencapture", &["-R", &format!("{},{},{},{}", r.x, r.y, r.width, r.height), "-x", &file]).await?;
        } else {
            Self::run("screencapture", &["-x", &file]).await?;
        }
        let png = tokio::fs::read(&file).await?;
        let _ = tokio::fs::remove_file(&file).await;
        Ok(png)
    }

    async fn display_size(&self) -> Result<(u32, u32)> {
        let output = Self::osascript(
            "tell app \"System Events\" to get the size of the first display"
        ).await?;
        let parts: Vec<u32> = output.split_whitespace()
            .filter_map(|s| s.parse().ok()).collect();
        Ok((parts.first().copied().unwrap_or(1920), parts.get(1).copied().unwrap_or(1080)))
    }
}

// ─── Input Simulation ────────────────────────────────────────

fn key_to_osascript(key: &Key) -> String {
    match key {
        Key::Char(c) => c.to_string(),
        Key::Named(n) => match n.to_lowercase().as_str() {
            "enter" | "return" => "return",
            "tab" => "tab",
            "escape" | "esc" => "escape",
            "space" => "space",
            "backspace" => "backspace",
            "delete" => "delete",
            "shift" => "shift",
            "control" | "ctrl" => "control",
            "alt" => "alt",
            "meta" | "cmd" | "command" => "command",
            "capslock" | "caps_lock" => "capslock",
            "up" => "up", "down" => "down", "left" => "left", "right" => "right",
            "home" => "home", "end" => "end",
            "pageup" | "page_up" => "pageup", "pagedown" | "page_down" => "pagedown",
            _ => n,
        }.to_string(),
    }
}

#[async_trait::async_trait]
impl InputSimulation for MacOSBackend {
    async fn mouse_move(&self, x: i32, y: i32) -> Result<()> {
        Self::osascript(&format!(
            "tell app \"System Events\" to set position of mouse to {{{}, {}}}", x, y
        )).await?;
        Ok(())
    }

    async fn mouse_click(&self, x: i32, y: i32, _button: MouseButton, clicks: u32) -> Result<()> {
        Self::mouse_move(self, x, y).await?;
        for _ in 0..clicks {
            Self::osascript("tell app \"System Events\" to click").await?;
        }
        Ok(())
    }

    async fn mouse_drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<()> {
        Self::osascript(&format!(
            "tell app \"System Events\" to drag from {{{}, {}}} to {{{}, {}}}",
            from.0, from.1, to.0, to.1
        )).await?;
        Ok(())
    }

    async fn mouse_scroll(&self, x: i32, y: i32, _delta_x: i32, delta_y: i32) -> Result<()> {
        Self::mouse_move(self, x, y).await?;
        let lines = delta_y.unsigned_abs();
        let dir = if delta_y > 0 { "up" } else { "down" };
        for _ in 0..lines {
            Self::osascript(&format!("tell app \"System Events\" to scroll 1 {}", dir)).await?;
        }
        Ok(())
    }

    async fn type_text(&self, text: &str) -> Result<()> {
        let escaped = text.replace('\\', "\\\\").replace('"', "\\\"");
        Self::osascript(&format!(
            "tell app \"System Events\" to keystroke \"{}\"", escaped
        )).await?;
        Ok(())
    }

    async fn key_press(&self, keys: &[Key], duration_ms: Option<u64>) -> Result<()> {
        let osa_keys: Vec<String> = keys.iter().map(key_to_osascript).collect();
        if keys.len() == 1 {
            let k = &osa_keys[0];
            if let Some(ms) = duration_ms {
                Self::osascript(&format!(
                    "tell app \"System Events\" to key down \"{}\"", k
                )).await?;
                tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
                Self::osascript(&format!(
                    "tell app \"System Events\" to key up \"{}\"", k
                )).await?;
            } else {
                Self::osascript(&format!(
                    "tell app \"System Events\" to keystroke \"{}\"", k
                )).await?;
            }
        } else {
            // Key combination: e.g. command+shift+a
            let combo = osa_keys.join(" ");
            Self::osascript(&format!(
                "tell app \"System Events\" to keystroke \"{}\" using {{}}", combo
            )).await?;
            // Actually use key down/up sequence for combinations
            for k in &osa_keys {
                Self::osascript(&format!("tell app \"System Events\" to key down \"{}\"", k)).await?;
            }
            if let Some(ms) = duration_ms {
                tokio::time::sleep(tokio::time::Duration::from_millis(ms)).await;
            } else {
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            }
            for k in osa_keys.iter().rev() {
                Self::osascript(&format!("tell app \"System Events\" to key up \"{}\"", k)).await?;
            }
        }
        Ok(())
    }
}

// ─── Accessibility Tree ──────────────────────────────────────

#[async_trait::async_trait]
impl AccessibilityTree for MacOSBackend {
    async fn get_tree(&self, _depth: Option<u32>) -> Result<A11yNode> {
        let (w, h) = self.display_size().await.unwrap_or((1920, 1080));
        let frontmost = Self::osascript(
            "tell app \"System Events\" to get name of first process whose frontmost is true"
        ).await.unwrap_or_else(|_| "macOS".into());

        Ok(A11yNode {
            ref_id: "root".into(),
            role: "application".into(),
            label: frontmost,
            bounds: Bounds { x: 0, y: 0, w, h },
            enabled: true, focused: true,
            children: vec![],
            actions: vec!["screenshot".into(), "click".into(), "type".into()],
        })
    }

    async fn find_element(&self, criteria: &FindCriteria) -> Result<Vec<A11yNode>> {
        // Simple window-level search via osascript
        let script = if let Some(label) = &criteria.label {
            format!("tell app \"System Events\" to get name of every window whose name contains \"{}\"", label)
        } else if let Some(role) = &criteria.role {
            format!("tell app \"System Events\" to get name of every process whose name contains \"{}\"", role)
        } else {
            return Ok(vec![]);
        };
        let output = Self::osascript(&script).await.unwrap_or_default();
        let (w, h) = self.display_size().await.unwrap_or((1920, 1080));
        Ok(output.split(", ").filter(|s| !s.is_empty()).map(|name| A11yNode {
            ref_id: name.to_string(),
            role: "window".into(),
            label: name.to_string(),
            bounds: Bounds { x: 0, y: 0, w, h },
            enabled: true, focused: false,
            children: vec![],
            actions: vec![],
        }).collect())
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
        Self::osascript(&format!(
            "tell app \"System Events\" to tell process \"{}\" to click", ref_id
        )).await?;
        Ok(())
    }

    async fn type_into_element(&self, _ref_id: &str, text: &str) -> Result<()> {
        self.type_text(text).await
    }
}

// ─── System Tools ────────────────────────────────────────────

#[async_trait::async_trait]
impl SystemTools for MacOSBackend {
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
        let output = tokio::process::Command::new("pbpaste").output().await?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    async fn clipboard_set(&self, text: &str) -> Result<()> {
        let mut child = tokio::process::Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()?;
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).await?;
        }
        child.wait().await?;
        Ok(())
    }

    async fn list_windows(&self) -> Result<Vec<WindowInfo>> {
        let output = Self::osascript(
            "tell app \"System Events\" to get name of every window of every process"
        ).await.unwrap_or_default();

        Ok(output.split(", ").filter(|s| !s.is_empty()).map(|title| WindowInfo {
            id: title.to_string(),
            title: title.to_string(),
            app_name: "".into(),
            bounds: Bounds { x: 0, y: 0, w: 0, h: 0 },
            minimized: false,
            focused: false,
        }).collect())
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        Self::osascript(&format!(
            "tell app \"System Events\" to set frontmost of every window whose name contains \"{}\" to true",
            window_id
        )).await?;
        Ok(())
    }
}
