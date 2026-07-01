use anyhow::{Context, Result};
use crate::platform::{
    A11yNode, AccessibilityTree, Bounds, CommandResult, FindCriteria, InputSimulation, Key,
    MouseButton, Rect, ScreenCapture, SystemTools, WindowInfo,
};

pub struct WindowsBackend;

impl WindowsBackend {
    pub fn new() -> Result<Self> {
        Ok(Self)
    }

    /// Helper: run a PowerShell command and return stdout.
    async fn ps(script: &str) -> Result<String> {
        let output = tokio::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", script])
            .output().await
            .with_context(|| format!("Failed to run PowerShell: {}", script))?;
        if !output.status.success() {
            anyhow::bail!("PowerShell failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Helper: run cmd.exe and return stdout.
    async fn cmd(args: &[&str]) -> Result<String> {
        let output = tokio::process::Command::new("cmd.exe")
            .args(args)
            .output().await
            .with_context(|| format!("Failed to run cmd: {:?}", args))?;
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

// ─── Screen Capture ──────────────────────────────────────────

#[async_trait::async_trait]
impl ScreenCapture for WindowsBackend {
    async fn capture_screen(&self, region: Option<Rect>) -> Result<Vec<u8>> {
        let file = format!("{}\\ocu-screenshot-{}.png", std::env::temp_dir().to_string_lossy(), std::process::id());
        if let Some(r) = region {
            Self::ps(&format!(
                "Add-Type -AssemblyName System.Drawing; \
                 $bmp = [System.Drawing.Bitmap]::FromScreen([System.Drawing.Rectangle]::FromXYWH({},{},{},{})); \
                 $bmp.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
                 $bmp.Dispose()",
                r.x, r.y, r.width, r.height, file
            )).await?;
        } else {
            Self::ps(&format!(
                "Add-Type -AssemblyName System.Drawing; \
                 $bmp = [System.Drawing.Bitmap]::FromScreen([System.Drawing.Rectangle]::FromXYWH(0,0,1920,1080)); \
                 $bmp.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
                 $bmp.Dispose()",
                file
            )).await?;
        }
        let png = tokio::fs::read(&file).await?;
        let _ = tokio::fs::remove_file(&file).await;
        Ok(png)
    }

    async fn display_size(&self) -> Result<(u32, u32)> {
        let output = Self::ps(
            "Add-Type -AssemblyName System.Windows.Forms; \
             [System.Windows.Forms.Screen]::PrimaryScreen.Bounds | \
             ForEach-Object { \"$($_.Width) $($_.Height)\" }"
        ).await?;
        let parts: Vec<u32> = output.split_whitespace()
            .filter_map(|s| s.parse().ok()).collect();
        Ok((parts.first().copied().unwrap_or(1920), parts.get(1).copied().unwrap_or(1080)))
    }
}

// ─── Input Simulation ────────────────────────────────────────

fn key_to_vk(key: &Key) -> &'static str {
    match key {
        Key::Char(c) => match c {
            'a'..='z' | 'A'..='Z' => c.to_uppercase().to_string().leak(),
            '0'..='9' => c.to_string().leak(),
            _ => c.to_string().leak(),
        },
        Key::Named(n) => match n.to_lowercase().as_str() {
            "enter" | "return" => "{Enter}",
            "tab" => "{Tab}",
            "escape" | "esc" => "{Escape}",
            "space" => " ",
            "backspace" => "{Backspace}",
            "delete" => "{Delete}",
            "shift" => "+",
            "control" | "ctrl" => "^",
            "alt" => "%",
            "meta" | "cmd" | "win" => "^{Esc}", // Windows key approximation
            "up" => "{Up}", "down" => "{Down}", "left" => "{Left}", "right" => "{Right}",
            "home" => "{Home}", "end" => "{End}",
            "pageup" | "page_up" => "{PgUp}", "pagedown" | "page_down" => "{PgDn}",
            _ => &format!("{{{}}}", n).leak(),
        }.leak(),
    }
}

#[async_trait::async_trait]
impl InputSimulation for WindowsBackend {
    async fn mouse_move(&self, x: i32, y: i32) -> Result<()> {
        Self::ps(&format!(
            "[System.Windows.Forms.Cursor]::Position = [System.Drawing.Point]::new({}, {})", x, y
        )).await?;
        Ok(())
    }

    async fn mouse_click(&self, x: i32, y: i32, _button: MouseButton, clicks: u32) -> Result<()> {
        Self::mouse_move(self, x, y).await?;
        for _ in 0..clicks {
            Self::ps("[System.Windows.Forms.SendKeys]::SendWait('{Click}')").await?;
        }
        Ok(())
    }

    async fn mouse_drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<()> {
        Self::ps(&format!(
            "[System.Windows.Forms.Cursor]::Position = [System.Drawing.Point]::new({}, {}); \
             [System.Windows.Forms.SendKeys]::SendWait('{{MouseDown}}'); \
             [System.Windows.Forms.Cursor]::Position = [System.Drawing.Point]::new({}, {}); \
             [System.Windows.Forms.SendKeys]::SendWait('{{MouseUp}}')",
            from.0, from.1, to.0, to.1
        )).await?;
        Ok(())
    }

    async fn mouse_scroll(&self, x: i32, y: i32, _delta_x: i32, delta_y: i32) -> Result<()> {
        Self::mouse_move(self, x, y).await?;
        let times = delta_y.unsigned_abs();
        for _ in 0..times {
            Self::ps("[System.Windows.Forms.SendKeys]::SendWait('{WheelDown}')").await?;
        }
        Ok(())
    }

    async fn type_text(&self, text: &str) -> Result<()> {
        let escaped = text.replace('\'', "''");
        Self::ps(&format!(
            "[System.Windows.Forms.SendKeys]::SendWait('{}')", escaped
        )).await?;
        Ok(())
    }

    async fn key_press(&self, keys: &[Key], duration_ms: Option<u64>) -> Result<()> {
        let vk_codes: Vec<&str> = keys.iter().map(key_to_vk).collect();
        if keys.len() == 1 {
            if let Some(ms) = duration_ms {
                Self::ps(&format!(
                    "$wshell = New-Object -ComObject wscript.shell; \
                     $wshell.SendKeys('{{{}}}'); \
                     Start-Sleep -Milliseconds {}; \
                     $wshell.SendKeys('{{{}}}')",
                    vk_codes[0], ms, vk_codes[0]
                )).await?;
            } else {
                Self::ps(&format!(
                    "[System.Windows.Forms.SendKeys]::SendWait('{}')", vk_codes[0]
                )).await?;
            }
        } else {
            // Key combination: e.g. ^+a = Ctrl+Shift+A
            let combo = vk_codes.join("");
            Self::ps(&format!(
                "[System.Windows.Forms.SendKeys]::SendWait('{}')", combo
            )).await?;
        }
        Ok(())
    }
}

// ─── Accessibility Tree ──────────────────────────────────────

#[async_trait::async_trait]
impl AccessibilityTree for WindowsBackend {
    async fn get_tree(&self, _depth: Option<u32>) -> Result<A11yNode> {
        let (w, h) = self.display_size().await.unwrap_or((1920, 1080));
        Ok(A11yNode {
            ref_id: "root".into(),
            role: "desktop".into(),
            label: "Windows Desktop".into(),
            bounds: Bounds { x: 0, y: 0, w, h },
            enabled: true, focused: true,
            children: vec![],
            actions: vec!["screenshot".into(), "click".into(), "type".into()],
        })
    }

    async fn find_element(&self, criteria: &FindCriteria) -> Result<Vec<A11yNode>> {
        let title_filter = criteria.label.as_deref().unwrap_or("");
        let output = Self::ps(&format!(
            "Add-Type @\"
            using System;
            using System.Runtime.InteropServices;
            public class Win32 {{
                [DllImport(\"user32.dll\")] public static extern IntPtr FindWindow(string cls, string win);
                [DllImport(\"user32.dll\")] public static extern int GetWindowText(IntPtr hWnd, System.Text.StringBuilder text, int count);
                [DllImport(\"user32.dll\")] public static extern IntPtr GetDesktopWindow();
            }}
\"@
            $desk = [Win32]::GetDesktopWindow()
            $hwnd = [Win32]::FindWindow([NullString]::Value, '*{}*')
            if ($hwnd -ne [IntPtr]::Zero) {{
                $sb = New-Object System.Text.StringBuilder 256
                [Win32]::GetWindowText($hwnd, $sb, 256)
                $sb.ToString()
            }}", title_filter
        )).await.unwrap_or_default();

        let (w, h) = self.display_size().await.unwrap_or((1920, 1080));
        Ok(if output.is_empty() {
            vec![]
        } else {
            vec![A11yNode {
                ref_id: output.clone(),
                role: "window".into(),
                label: output,
                bounds: Bounds { x: 0, y: 0, w, h },
                enabled: true, focused: false,
                children: vec![],
                actions: vec![],
            }]
        })
    }

    async fn get_element_info(&self, ref_id: &str) -> Result<A11yNode> {
        let nodes = self.find_element(&FindCriteria {
            role: None,
            label: Some(ref_id.to_string()),
            text: None,
        }).await?;
        nodes.into_iter().next().context("Element not found")
    }

    async fn click_element(&self, ref_id: &str) -> Result<()> {
        Self::ps(&format!(
            "Add-Type @\"
            using System;
            using System.Runtime.InteropServices;
            public class Win32 {{
                [DllImport(\"user32.dll\")] public static extern IntPtr FindWindow(string cls, string win);
                [DllImport(\"user32.dll\")] public static extern bool SetForegroundWindow(IntPtr hWnd);
            }}
\"@
            $hwnd = [Win32]::FindWindow([NullString]::Value, '*{}*')
            if ($hwnd -ne [IntPtr]::Zero) {{ [Win32]::SetForegroundWindow($hwnd) }}", ref_id
        )).await?;
        Ok(())
    }

    async fn type_into_element(&self, _ref_id: &str, text: &str) -> Result<()> {
        self.type_text(text).await
    }
}

// ─── System Tools ────────────────────────────────────────────

#[async_trait::async_trait]
impl SystemTools for WindowsBackend {
    async fn run_command(&self, command: &str, timeout_secs: u64) -> Result<CommandResult> {
        let output = tokio::time::timeout(
            tokio::time::Duration::from_secs(timeout_secs),
            tokio::process::Command::new("cmd.exe")
                .args(["/C", command])
                .output(),
        ).await.map_err(|_| anyhow::anyhow!("Command timed out after {}s", timeout_secs))??;

        Ok(CommandResult {
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    async fn clipboard_get(&self) -> Result<String> {
        let output = Self::ps("Get-Clipboard").await?;
        Ok(output)
    }

    async fn clipboard_set(&self, text: &str) -> Result<()> {
        let escaped = text.replace('\'', "''");
        Self::ps(&format!("Set-Clipboard -Value '{}'", escaped)).await?;
        Ok(())
    }

    async fn list_windows(&self) -> Result<Vec<WindowInfo>> {
        let output = Self::ps(
            "Add-Type @\"
            using System;
            using System.Runtime.InteropServices;
            using System.Text;
            public class Win32 {{
                [DllImport(\"user32.dll\")] public static extern IntPtr GetForegroundWindow();
                [DllImport(\"user32.dll\")] public static extern int GetWindowText(IntPtr hWnd, StringBuilder text, int count);
                [DllImport(\"user32.dll\")] public static extern IntPtr GetWindow(IntPtr hWnd, int uCmd);
                public const int GW_HWNDNEXT = 2;
                [DllImport(\"user32.dll\")] public static extern bool IsWindowVisible(IntPtr hWnd);
            }}
\"@
            $results = @()
            $hwnd = [Win32]::GetForegroundWindow()
            for ($i = 0; $i -lt 100; $i++) {{
                if ($hwnd -eq [IntPtr]::Zero) {{ break }}
                if ([Win32]::IsWindowVisible($hwnd)) {{
                    $sb = New-Object System.Text.StringBuilder 256
                    [Win32]::GetWindowText($hwnd, $sb, 256)
                    $title = $sb.ToString()
                    if ($title) {{ $results += $title }}
                }}
                $hwnd = [Win32]::GetWindow($hwnd, [Win32]::GW_HWNDNEXT)
            }}
            $results -join \"`n\""
        ).await.unwrap_or_default();

        Ok(output.lines().filter(|s| !s.is_empty()).map(|title| WindowInfo {
            id: title.to_string(),
            title: title.to_string(),
            app_name: "".into(),
            bounds: Bounds { x: 0, y: 0, w: 0, h: 0 },
            minimized: false,
            focused: false,
        }).collect())
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        Self::ps(&format!(
            "Add-Type @\"
            using System;
            using System.Runtime.InteropServices;
            public class Win32 {{
                [DllImport(\"user32.dll\")] public static extern IntPtr FindWindow(string cls, string win);
                [DllImport(\"user32.dll\")] public static extern bool SetForegroundWindow(IntPtr hWnd);
                [DllImport(\"user32.dll\")] public static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);
            }}
\"@
            $hwnd = [Win32]::FindWindow([NullString]::Value, '*{}*')
            if ($hwnd -ne [IntPtr]::Zero) {{
                [Win32]::ShowWindow($hwnd, 9)
                [Win32]::SetForegroundWindow($hwnd)
            }}", window_id
        )).await?;
        Ok(())
    }
}
