//! Session-side I/O for the Conductor. Creates a per-session "bus" directory under
//! XDG state and writes the generated hook scripts + agent config into it; the panes
//! point each agent at it via launch flags + env vars (see `tcode_core::conductor`
//! for the pure content + wiring logic). Nothing is written into the user's real
//! `~/.claude`/`~/.codex` or their repository.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use tcode_core::conductor as core;

/// Distinguishes concurrent sessions in one process (the Stack of sessions) so each
/// grid gets its own bus dir.
static SESSION_SEQ: AtomicU64 = AtomicU64::new(0);

/// A per-session coordination bus: a directory holding the shared event ledger
/// (`events.jsonl`, written by the hooks) plus the generated hook/config files.
pub struct SessionBus {
    dir: PathBuf,
}

impl SessionBus {
    /// Create a fresh bus dir under XDG state and write the hook scripts + agent
    /// config. Returns `None` on any I/O failure — coordination is then silently
    /// skipped (the agents still launch, just unwired), so a read-only state dir can
    /// never stop a session from opening.
    pub fn create() -> Option<SessionBus> {
        let key = format!(
            "{}-{}",
            std::process::id(),
            SESSION_SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let dir = bus_root().join(key);
        std::fs::create_dir_all(&dir).ok()?;
        write_script(&dir.join("record.sh"), core::record_hook_script())?;
        write_script(&dir.join("inject.sh"), core::inject_hook_script())?;
        let bus = dir.to_str()?.to_string();
        std::fs::write(dir.join("claude-settings.json"), core::claude_settings_json(&bus)).ok()?;
        std::fs::write(dir.join("codex-mcp.json"), core::codex_mcp_json()).ok()?;
        Some(SessionBus { dir })
    }

    /// Absolute path to the bus dir, as a string (for env + wiring). `None` only if
    /// the path isn't valid UTF-8, in which case the agent launches unwired.
    pub fn dir_str(&self) -> Option<&str> {
        self.dir.to_str()
    }
}

/// `$XDG_STATE_HOME/tcode/conductor` or `~/.local/state/tcode/conductor`, mirroring
/// the XDG resolution in `config_dir` — never a CWD-relative path.
fn bus_root() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .map(|h| h.join(".local").join("state"))
        })
        .unwrap_or_else(std::env::temp_dir);
    base.join("tcode").join("conductor")
}

/// Write a hook script and mark it executable (the agents run it as a command).
fn write_script(path: &Path, content: &str) -> Option<()> {
    std::fs::write(path, content).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).ok()?;
    }
    Some(())
}
