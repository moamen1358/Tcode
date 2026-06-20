//! User configuration, loaded from `~/.config/tessera/config.toml`.
//! Every field has a default, so a missing or partial file is fine.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: String,
    pub font_size: u32,
    pub startup_command: String,
    /// Persist clipboard history to disk across restarts. Off by default so copied
    /// secrets are not stored unless the user explicitly opts in.
    pub clipboard_persist: bool,
    /// Whole-UI zoom multiplier (1.0 = 100%): scales every font/terminal together.
    pub scale: f64,
    pub theme: Theme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
            clipboard_persist: false,
            scale: 1.0,
            theme: Theme::default(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            background: "#1a1b26".into(),
            foreground: "#c0caf5".into(),
            accent: "#ff9e64".into(),
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
        match toml::from_str::<Config>(s) {
            Ok(mut c) => {
                c.clamp();
                c
            }
            Err(e) => {
                eprintln!("tessera: config parse error ({e}); using defaults");
                Config::default()
            }
        }
    }

    /// Bounds for the UI scale (50%–300%).
    pub const MIN_SCALE: f64 = 0.5;
    pub const MAX_SCALE: f64 = 3.0;

    /// Keep runtime-adjustable values in sane ranges (a hand-edited or corrupt
    /// config can't push the font/scale to something unusable).
    pub fn clamp(&mut self) {
        self.font_size = self.font_size.clamp(4, 96);
        if !self.scale.is_finite() {
            self.scale = 1.0;
        }
        self.scale = self.scale.clamp(Self::MIN_SCALE, Self::MAX_SCALE);
    }

    /// Persist to the standard config path (creating the dir as needed).
    pub fn save(&self) {
        let Ok(text) = toml::to_string_pretty(self) else {
            return;
        };
        let path = config_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if write_private(&path, text.as_bytes()).is_ok() {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
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
        assert!(!c.clipboard_persist); // off by default
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
