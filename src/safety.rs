use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tracing::info;

/// Severity level of a tool action — determines whether confirmation is needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    /// Read-only, always allowed.
    Info,
    /// Potentially modifies state (clicks, key presses). Configurable gate.
    Normal,
    /// Destructive (shell command, clipboard write). Always gated if enabled.
    Dangerous,
}

/// A single audit log entry.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AuditEntry {
    pub id: u64,
    pub session_id: String,
    pub tool: String,
    pub args: serde_json::Value,
    pub result: serde_json::Value,
    pub duration_ms: u64,
    pub timestamp: String,
}

/// Ring-buffer audit log.
pub struct AuditLog {
    entries: Mutex<VecDeque<AuditEntry>>,
    capacity: usize,
    next_id: AtomicU64,
}

impl AuditLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity)),
            capacity,
            next_id: AtomicU64::new(1),
        }
    }

    pub async fn record(&self, session_id: &str, tool: &str, args: &serde_json::Value, result: &serde_json::Value, duration_ms: u64) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let timestamp = chrono::Utc::now().to_rfc3339();
        let entry = AuditEntry {
            id,
            session_id: session_id.to_string(),
            tool: tool.to_string(),
            args: args.clone(),
            result: result.clone(),
            duration_ms,
            timestamp,
        };
        let mut entries = self.entries.lock().await;
        if entries.len() >= self.capacity {
            entries.pop_front();
        }
        entries.push_back(entry);
        id
    }

    pub async fn recent(&self, n: usize) -> Vec<AuditEntry> {
        let entries = self.entries.lock().await;
        entries.iter().rev().take(n).cloned().collect()
    }
}

/// Sandbox — checks paths, commands, and network hosts against allowlists.
pub struct Sandbox {
    pub enabled: bool,
    pub allowed_paths: Vec<String>,
    pub allowed_network: Vec<String>,
    pub allowed_commands: Vec<String>,
}

impl Sandbox {
    pub fn new(enabled: bool, allowed_paths: Vec<String>, allowed_network: Vec<String>, allowed_commands: Vec<String>) -> Self {
        Self { enabled, allowed_paths, allowed_network, allowed_commands }
    }

    pub fn check_command(&self, command: &str) -> Result<(), String> {
        if !self.enabled || self.allowed_commands.is_empty() {
            return Ok(());
        }
        let cmd_name = command.split_whitespace().next().unwrap_or("");
        if self.allowed_commands.iter().any(|a| cmd_name == a || command.starts_with(a)) {
            return Ok(());
        }
        Err(format!("Command '{}' is not in the allowed commands list", cmd_name))
    }

    pub fn check_network(&self, host: &str) -> Result<(), String> {
        if !self.enabled || self.allowed_network.is_empty() {
            return Ok(());
        }
        if self.allowed_network.iter().any(|pattern| {
            if pattern.starts_with("*.") {
                host.ends_with(&pattern[1..])
            } else {
                host == pattern.as_str()
            }
        }) {
            return Ok(());
        }
        Err(format!("Host '{}' is not in the network allowlist", host))
    }

    pub fn check_path(&self, path: &str) -> Result<(), String> {
        if !self.enabled || self.allowed_paths.is_empty() {
            return Ok(());
        }
        let canonical = std::path::Path::new(path);
        if self.allowed_paths.iter().any(|p| canonical.starts_with(p)) {
            return Ok(());
        }
        Err(format!("Path '{}' is not in the allowed paths list", path))
    }
}

/// Rate limiter — enforces a maximum number of actions per time window.
pub struct RateLimiter {
    enabled: bool,
    max_actions: u32,
    window_secs: u64,
    timestamps: Mutex<VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new(enabled: bool, max_actions: u32, window_secs: u64) -> Self {
        Self { enabled, max_actions, window_secs, timestamps: Mutex::new(VecDeque::new()) }
    }

    pub async fn check(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        let mut timestamps = self.timestamps.lock().await;
        let now = Instant::now();
        let cutoff = now - Duration::from_secs(self.window_secs);

        while timestamps.front().map_or(false, |t| *t < cutoff) {
            timestamps.pop_front();
        }

        if timestamps.len() as u32 >= self.max_actions {
            let oldest = timestamps.front().copied().unwrap();
            let retry_after = (oldest + Duration::from_secs(self.window_secs)).saturating_duration_since(now);
            return Err(format!("Rate limit exceeded. Max {} actions per {}s. Retry after {}ms.",
                self.max_actions, self.window_secs, retry_after.as_millis()));
        }

        timestamps.push_back(now);
        Ok(())
    }
}

/// Confirmation gate — intercepts destructive actions and prompts the user.
pub struct ConfirmationGate {
    pub enabled: bool,
}

impl ConfirmationGate {
    pub const fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    /// Returns true if the action is approved.
    pub async fn confirm(&self, tool: &str, args: &serde_json::Value, severity: &Severity) -> bool {
        if !self.enabled || *severity == Severity::Info {
            return true;
        }
        // Print prompt to stderr (clean JSON-RPC on stdout)
        eprintln!("\n⚠️  Confirmation required for tool: {}", tool);
        eprintln!("   Args: {}", serde_json::to_string(args).unwrap_or_default());
        eprint!("   Proceed? (y/N): ");

        // Read a single line from stdin
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            eprintln!("   Failed to read confirmation. Denying.");
            return false;
        }

        let approved = matches!(input.trim().to_lowercase().as_str(), "y" | "yes" | "Y" | "YES");
        if approved {
            info!("Action approved: {}", tool);
        } else {
            info!("Action denied: {}", tool);
        }
        approved
    }
}

/// Per-session state for session isolation.
/// Each MCP session (e.g., each SSE connection) gets its own `Session` instance
/// with independent audit log and rate limiter.
pub struct Session {
    pub id: String,
    pub audit_log: AuditLog,
    pub rate_limiter: RateLimiter,
}

impl Session {
    pub fn new(id: String, audit_capacity: usize, rate_limit_enabled: bool, max_actions: u32, window_secs: u64) -> Self {
        Self {
            id,
            audit_log: AuditLog::new(audit_capacity),
            rate_limiter: RateLimiter::new(rate_limit_enabled, max_actions, window_secs),
        }
    }
}

/// Alias for thread-safe shared state.
pub type SharedSession = Arc<Session>;
pub type SharedSandbox = Arc<Sandbox>;
pub type SharedGate = Arc<ConfirmationGate>;
