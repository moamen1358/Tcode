//! Named workspace sessions. Each session remembers its root folder, the number
//! of terminal panes, and the open editor files, persisted as a TOML file under
//! `~/.config/loom/sessions/` so Loom can reopen where you left off.
//! (Split sizes + per-terminal working dirs are modeled below but not captured
//! yet — reserved for a later pass.)
//!
//! This is pure data + disk I/O (no GTK); the UI lives in the `loom` crate
//! (`session_picker` for the startup screen, the titlebar switcher in `app`).

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::config_dir;

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
    /// Reserved (not captured yet): split divider ratios for exact split sizes.
    #[serde(default)]
    pub divisors: Vec<f64>,
    /// Reserved (not captured yet): per-terminal working directories.
    #[serde(default)]
    pub cwds: Vec<PathBuf>,
    /// Open editor file paths, in tab order.
    #[serde(default)]
    pub files: Vec<PathBuf>,
    /// Index into `files` of the active tab.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<usize>,
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
            cwds: Vec::new(),
            files: Vec::new(),
            active: None,
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
        if std::fs::write(&path, text).is_ok() {
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

/// Directory holding session files: `~/.config/loom/sessions/`.
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
    let path = sessions_dir().join(format!("{id}.toml"));
    let text = std::fs::read_to_string(&path).ok()?;
    let mut s = toml::from_str::<Session>(&text).ok()?;
    s.id = id.to_string();
    Some(s)
}

/// A unique id from the current time (nanoseconds, hex).
fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}")
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
}
