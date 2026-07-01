use anyhow::Result;

/// Rectangle on screen
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Mouse button enum
#[derive(Debug, Clone, Copy)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

/// Key representation for keyboard input
#[derive(Debug, Clone)]
pub enum Key {
    Char(char),
    Named(String), // e.g. "Enter", "Tab", "Escape", "Shift", "Cmd", "Ctrl"
}

/// Accessibility tree node
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct A11yNode {
    pub ref_id: String,
    pub role: String,
    pub label: String,
    pub bounds: Bounds,
    pub enabled: bool,
    pub focused: bool,
    pub children: Vec<A11yNode>,
    pub actions: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Criteria for finding elements in the accessibility tree
#[derive(Debug, Clone, Default)]
pub struct FindCriteria {
    pub role: Option<String>,
    pub label: Option<String>,
    pub text: Option<String>,
}

/// Window information for window management
#[derive(Debug, Clone, serde::Serialize)]
pub struct WindowInfo {
    pub id: String,
    pub title: String,
    pub app_name: String,
    pub bounds: Bounds,
    pub minimized: bool,
    pub focused: bool,
}

/// System utilities trait (command execution, clipboard, window management)
#[async_trait::async_trait]
pub trait SystemTools: Send + Sync {
    /// Run a shell command and return stdout, stderr, exit code
    async fn run_command(&self, command: &str, timeout_secs: u64) -> Result<CommandResult>;
    /// Read current clipboard content
    async fn clipboard_get(&self) -> Result<String>;
    /// Write text to clipboard
    async fn clipboard_set(&self, text: &str) -> Result<()>;
    /// List all visible windows
    async fn list_windows(&self) -> Result<Vec<WindowInfo>>;
    /// Focus/bring-to-front a window by ID
    async fn focus_window(&self, window_id: &str) -> Result<()>;
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Screen capture trait
#[async_trait::async_trait]
pub trait ScreenCapture: Send + Sync {
    async fn capture_screen(&self, region: Option<Rect>) -> Result<Vec<u8>>;
    async fn display_size(&self) -> Result<(u32, u32)>;
}

/// Input simulation trait
#[async_trait::async_trait]
pub trait InputSimulation: Send + Sync {
    async fn mouse_move(&self, x: i32, y: i32) -> Result<()>;
    async fn mouse_click(&self, x: i32, y: i32, button: MouseButton, clicks: u32) -> Result<()>;
    async fn mouse_drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<()>;
    async fn mouse_scroll(&self, x: i32, y: i32, delta_x: i32, delta_y: i32) -> Result<()>;
    async fn type_text(&self, text: &str) -> Result<()>;
    async fn key_press(&self, keys: &[Key], duration_ms: Option<u64>) -> Result<()>;
}

/// Accessibility tree trait
#[async_trait::async_trait]
pub trait AccessibilityTree: Send + Sync {
    async fn get_tree(&self, depth: Option<u32>) -> Result<A11yNode>;
    async fn find_element(&self, criteria: &FindCriteria) -> Result<Vec<A11yNode>>;
    async fn get_element_info(&self, ref_id: &str) -> Result<A11yNode>;
    async fn click_element(&self, ref_id: &str) -> Result<()>;
    async fn type_into_element(&self, ref_id: &str, text: &str) -> Result<()>;
}

/// Combined platform backend
#[async_trait::async_trait]
pub trait PlatformBackend: ScreenCapture + InputSimulation + AccessibilityTree + SystemTools {}

#[async_trait::async_trait]
impl<T: ScreenCapture + InputSimulation + AccessibilityTree + SystemTools> PlatformBackend for T {}

// Platform-specific implementations
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

/// Runtime platform enum that delegates to the appropriate backend
pub enum Platform {
    #[cfg(target_os = "macos")]
    MacOS(macos::MacOSBackend),
    #[cfg(target_os = "windows")]
    Windows(windows::WindowsBackend),
    #[cfg(target_os = "linux")]
    Linux(linux::LinuxBackend),
}

impl Platform {
    pub fn new() -> Result<Self> {
        #[cfg(target_os = "macos")]
        {
            Ok(Self::MacOS(macos::MacOSBackend::new()?))
        }
        #[cfg(target_os = "windows")]
        {
            Ok(Self::Windows(windows::WindowsBackend::new()?))
        }
        #[cfg(target_os = "linux")]
        {
            Ok(Self::Linux(linux::LinuxBackend::new()?))
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        {
            compile_error!("Unsupported target OS. Supported: macOS, Windows, Linux");
        }
    }
}

#[async_trait::async_trait]
impl ScreenCapture for Platform {
    async fn capture_screen(&self, region: Option<Rect>) -> Result<Vec<u8>> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.capture_screen(region).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.capture_screen(region).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.capture_screen(region).await,
        }
    }

    async fn display_size(&self) -> Result<(u32, u32)> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.display_size().await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.display_size().await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.display_size().await,
        }
    }
}

#[async_trait::async_trait]
impl InputSimulation for Platform {
    async fn mouse_move(&self, x: i32, y: i32) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.mouse_move(x, y).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.mouse_move(x, y).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.mouse_move(x, y).await,
        }
    }

    async fn mouse_click(&self, x: i32, y: i32, button: MouseButton, clicks: u32) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.mouse_click(x, y, button, clicks).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.mouse_click(x, y, button, clicks).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.mouse_click(x, y, button, clicks).await,
        }
    }

    async fn mouse_drag(&self, from: (i32, i32), to: (i32, i32)) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.mouse_drag(from, to).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.mouse_drag(from, to).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.mouse_drag(from, to).await,
        }
    }

    async fn mouse_scroll(&self, x: i32, y: i32, delta_x: i32, delta_y: i32) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.mouse_scroll(x, y, delta_x, delta_y).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.mouse_scroll(x, y, delta_x, delta_y).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.mouse_scroll(x, y, delta_x, delta_y).await,
        }
    }

    async fn type_text(&self, text: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.type_text(text).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.type_text(text).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.type_text(text).await,
        }
    }

    async fn key_press(&self, keys: &[Key], duration_ms: Option<u64>) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.key_press(keys, duration_ms).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.key_press(keys, duration_ms).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.key_press(keys, duration_ms).await,
        }
    }
}

#[async_trait::async_trait]
impl AccessibilityTree for Platform {
    async fn get_tree(&self, depth: Option<u32>) -> Result<A11yNode> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.get_tree(depth).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.get_tree(depth).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.get_tree(depth).await,
        }
    }

    async fn find_element(&self, criteria: &FindCriteria) -> Result<Vec<A11yNode>> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.find_element(criteria).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.find_element(criteria).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.find_element(criteria).await,
        }
    }

    async fn get_element_info(&self, ref_id: &str) -> Result<A11yNode> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.get_element_info(ref_id).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.get_element_info(ref_id).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.get_element_info(ref_id).await,
        }
    }

    async fn click_element(&self, ref_id: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.click_element(ref_id).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.click_element(ref_id).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.click_element(ref_id).await,
        }
    }

    async fn type_into_element(&self, ref_id: &str, text: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.type_into_element(ref_id, text).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.type_into_element(ref_id, text).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.type_into_element(ref_id, text).await,
        }
    }
}

#[async_trait::async_trait]
impl SystemTools for Platform {
    async fn run_command(&self, command: &str, timeout_secs: u64) -> Result<CommandResult> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.run_command(command, timeout_secs).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.run_command(command, timeout_secs).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.run_command(command, timeout_secs).await,
        }
    }

    async fn clipboard_get(&self) -> Result<String> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.clipboard_get().await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.clipboard_get().await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.clipboard_get().await,
        }
    }

    async fn clipboard_set(&self, text: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.clipboard_set(text).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.clipboard_set(text).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.clipboard_set(text).await,
        }
    }

    async fn list_windows(&self) -> Result<Vec<WindowInfo>> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.list_windows().await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.list_windows().await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.list_windows().await,
        }
    }

    async fn focus_window(&self, window_id: &str) -> Result<()> {
        match self {
            #[cfg(target_os = "macos")]
            Self::MacOS(b) => b.focus_window(window_id).await,
            #[cfg(target_os = "windows")]
            Self::Windows(b) => b.focus_window(window_id).await,
            #[cfg(target_os = "linux")]
            Self::Linux(b) => b.focus_window(window_id).await,
        }
    }
}
