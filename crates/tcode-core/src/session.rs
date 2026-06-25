//! Named workspace sessions. Each session remembers its root folder, the
//! terminal layout (pane count + split sizes), and the open editor files,
//! persisted as a TOML file under `~/.config/tcode/sessions/` so Tcode can reopen
//! where you left off. (Per-terminal working dirs are modeled below but not
//! captured yet — reserved for a later pass.)
//!
//! This is pure data + disk I/O (no GTK); the UI lives in the `tcode` crate
//! (`session_picker` for the startup screen, the titlebar switcher in `app`).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::config_dir;

static SESSION_ID_SEQ: AtomicU64 = AtomicU64::new(0);

/// Hard cap on panes per session, matching the grid's own clamp. Bounds a
/// hand-edited or corrupt `panes` value on load so the picker, the persisted
/// model, and the grid that actually gets built all agree.
const MAX_PANES: usize = 16;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Session {
    /// Filename stem — a stable id that survives renames. Derived from the file
    /// path, so it is not part of the serialized body.
    #[serde(skip)]
    pub id: String,
    /// Display name (defaults to the root folder's name; user-renamable).
    pub name: String,
    /// Root folder: the sidebar root and where terminals start.
    pub root: PathBuf,
    /// Number of terminal panes.
    pub panes: usize,
    /// Split divider positions as ratios of the container (in paned order), so
    /// the terminals reopen resized exactly how you left them.
    #[serde(default)]
    pub divisors: Vec<f64>,
    /// Open editor file paths, in tab order.
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// Index into `files` of the active tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<usize>,
    /// Terminals' share (0..1) of the center split (terminals | editor), so a
    /// resized editor reopens at the same width. `None` when no file was open.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_split: Option<f64>,
    /// Sidebar width in pixels.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_width: Option<i32>,
}

impl Session {
    /// A fresh single-terminal session rooted at `root`, named for its folder.
    pub fn new(root: PathBuf) -> Session {
        let name = root
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| root.display().to_string());
        Session {
            id: new_id(),
            name,
            root,
            panes: 1,
            divisors: Vec::new(),
            files: Vec::new(),
            active: None,
            editor_split: None,
            sidebar_width: None,
        }
    }

    /// The session's TOML file path.
    pub fn path(&self) -> PathBuf {
        sessions_dir().join(format!("{}.toml", self.id))
    }

    /// Persist this session to disk (creating the sessions dir as needed).
    /// Writing also bumps the file's mtime, which `list` uses as "last used".
    pub fn save(&self) {
        let Ok(text) = toml::to_string_pretty(self) else {
            return;
        };
        let dir = sessions_dir();
        let _ = std::fs::create_dir_all(&dir);
        let path = self.path();
        if write_private(&path, text.as_bytes()).is_ok() {
            // Session files record the open-file paths; keep them owner-only.
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }

    /// Remove this session's file.
    pub fn delete(&self) {
        let _ = std::fs::remove_file(self.path());
    }
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(path)?;
    file.write_all(bytes)
}

/// Directory holding session files: `~/.config/tcode/sessions/`.
pub fn sessions_dir() -> PathBuf {
    config_dir().join("sessions")
}

/// All saved sessions, most-recently-used first (by file mtime).
pub fn list() -> Vec<Session> {
    let Ok(entries) = std::fs::read_dir(sessions_dir()) else {
        return Vec::new();
    };
    let mut out: Vec<(SystemTime, Session)> = Vec::new();
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(mut s) = toml::from_str::<Session>(&text) else {
            continue;
        };
        s.id = path
            .file_stem()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();
        s.panes = s.panes.clamp(1, MAX_PANES);
        let mtime = e
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(UNIX_EPOCH);
        out.push((mtime, s));
    }
    out.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime)); // newest first
    out.into_iter().map(|(_, s)| s).collect()
}

/// Load a single session by id, if it exists.
pub fn load(id: &str) -> Option<Session> {
    if !valid_session_id(id) {
        return None;
    }
    let path = sessions_dir().join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&path).ok()?;
    let mut s = toml::from_str::<Session>(&text).ok()?;
    s.id = id.to_string();
    s.panes = s.panes.clamp(1, MAX_PANES);
    Some(s)
}

/// A unique id from time + process + an in-process sequence, encoded as hex.
fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = SESSION_ID_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:x}{:x}{seq:x}", std::process::id())
}

fn valid_session_id(id: &str) -> bool {
    !id.is_empty() && id.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_toml() {
        let mut s = Session::new(PathBuf::from("/tmp/proj"));
        s.panes = 4;
        s.divisors = vec![0.5, 0.33];
        s.files = vec![
            PathBuf::from("/tmp/proj/a.rs"),
            PathBuf::from("/tmp/proj/b.rs"),
        ];
        s.active = Some(1);
        let text = toml::to_string_pretty(&s).unwrap();
        let back: Session = toml::from_str(&text).unwrap();
        assert_eq!(back.name, "proj");
        assert_eq!(back.panes, 4);
        assert_eq!(back.divisors, vec![0.5, 0.33]);
        assert_eq!(back.files.len(), 2);
        assert_eq!(back.active, Some(1));
    }

    #[test]
    fn new_is_named_for_its_folder() {
        let s = Session::new(PathBuf::from("/home/u/coding_Space"));
        assert_eq!(s.name, "coding_Space");
        assert_eq!(s.panes, 1);
        assert!(s.files.is_empty());
    }

    #[test]
    fn session_ids_are_hex_and_reject_paths() {
        let a = Session::new(PathBuf::from("/tmp/a")).id;
        let b = Session::new(PathBuf::from("/tmp/b")).id;
        assert_ne!(a, b);
        assert!(valid_session_id(&a));
        assert!(!valid_session_id(""));
        assert!(!valid_session_id("../secret"));
        assert!(!valid_session_id("abc/def"));
        assert!(!valid_session_id("abc.toml"));
    }
}
