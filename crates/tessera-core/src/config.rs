//! User configuration, loaded from `~/.config/tessera/config.toml`.
//! Every field has a default, so a missing or partial file is fine.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub font: String,
    pub font_size: u32,
    pub gap: u32,
    pub startup_command: String,
    pub theme: Theme,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub palette: Vec<String>,
}

const DEFAULT_PALETTE: [&str; 16] = [
    "#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
    "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8",
];

impl Default for Config {
    fn default() -> Self {
        Config {
            font: "monospace".into(),
            font_size: 11,
            gap: 8,
            startup_command: String::new(),
            theme: Theme::default(),
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            background: "#1e1e2e".into(),
            foreground: "#cdd6f4".into(),
            accent: "#89b4fa".into(),
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

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".config")
        });
    base.join("tessera").join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_sane() {
        let c = Config::default();
        assert_eq!(c.font_size, 11);
        assert_eq!(c.gap, 8);
        assert_eq!(c.theme.palette.len(), 16);
        assert!(c.startup_command.is_empty());
    }

    #[test]
    fn partial_toml_merges_with_defaults() {
        let c = Config::from_str_or_default("font_size = 14\nstartup_command = \"claude\"");
        assert_eq!(c.font_size, 14);
        assert_eq!(c.startup_command, "claude");
        assert_eq!(c.font, "monospace");
        assert_eq!(c.gap, 8);
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
        assert_eq!(c.theme.background, "#1e1e2e");
    }
}
