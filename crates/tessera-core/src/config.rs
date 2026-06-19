//! User configuration, loaded from `~/.config/tessera/config.toml`.
//! Every field has a default, so a missing or partial file is fine.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: String,
    pub font_size: u32,
    pub startup_command: String,
    /// Persist clipboard history to disk across restarts. On by default; set to
    /// `false` to keep history only for the running session (nothing written to
    /// disk) — useful if you don't want copied secrets stored.
    pub clipboard_persist: bool,
    pub theme: Theme,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    /// Sidebar / tab-bar / titlebar background (a shade off `background`).
    pub surface: String,
    /// Separator / divider / border color.
    pub border: String,
    pub palette: Vec<String>,
}

// Tokyo Night.
const DEFAULT_PALETTE: [&str; 16] = [
    "#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
    "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5",
];

impl Default for Config {
    fn default() -> Self {
        Config {
            font: "Martian Mono".into(),
            font_size: 11,
            startup_command: String::new(),
            clipboard_persist: true,
            theme: Theme::default(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            background: "#1a1b26".into(),
            foreground: "#c0caf5".into(),
            accent: "#7aa2f7".into(),
            surface: "#16161e".into(),
            border: "#2f3549".into(),
            palette: DEFAULT_PALETTE.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Config {
    /// Load from the standard config path, falling back to defaults.
    pub fn load() -> Self {
        Self::from_path(&config_path())
    }

    /// Load from a specific path, falling back to defaults if missing/unreadable.
    pub fn from_path(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::from_str_or_default(&s),
            Err(_) => Config::default(),
        }
    }

    /// Parse TOML, falling back to defaults (with a warning) on parse error.
    /// Missing fields are filled from defaults via `#[serde(default)]`.
    pub fn from_str_or_default(s: &str) -> Config {
        match toml::from_str(s) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("tessera: config parse error ({e}); using defaults");
                Config::default()
            }
        }
    }
}

/// Tessera's base config directory: `$XDG_CONFIG_HOME/tessera` or
/// `~/.config/tessera`. Shared by the config file and saved sessions.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".config")
        });
    base.join("tessera")
}

fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = Config::default();
        assert_eq!(c.font_size, 11);
        assert_eq!(c.theme.palette.len(), 16);
        assert!(c.startup_command.is_empty());
        assert!(c.clipboard_persist); // on by default
    }

    #[test]
    fn partial_toml_merges_with_defaults() {
        let c = Config::from_str_or_default("font_size = 14\nstartup_command = \"claude\"");
        assert_eq!(c.font_size, 14);
        assert_eq!(c.startup_command, "claude");
        assert_eq!(c.font, "Martian Mono");
    }

    #[test]
    fn bad_toml_falls_back_to_defaults() {
        let c = Config::from_str_or_default("= = = not toml");
        assert_eq!(c.font_size, 11);
    }

    #[test]
    fn partial_theme_merges() {
        let c = Config::from_str_or_default("[theme]\naccent = \"#ff0000\"");
        assert_eq!(c.theme.accent, "#ff0000");
        assert_eq!(c.theme.background, "#1a1b26");
    }
}
