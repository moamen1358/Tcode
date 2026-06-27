//! User configuration, loaded from `~/.config/tcode/config.toml`.
//! Every field has a default, so a missing or partial file is fine.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::agents::Agent;

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
    /// Commands run to auto-launch each coding agent in a session's panes.
    pub agents: Agents,
    /// Multi-agent coordination (the "Conductor"): awareness + delegation wiring.
    pub coordination: Coordination,
}

/// Multi-agent coordination. When Tcode launches agent panes, it wires them to a
/// shared per-session bus so they're aware of each other's edits and a Claude pane
/// can delegate to Codex — all via launch flags/env, never the user's real config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Coordination {
    /// Auto-wire coordination when launching agent panes. On by default.
    pub enabled: bool,
}

impl Default for Coordination {
    fn default() -> Self {
        Coordination { enabled: true }
    }
}

/// The shell command run to launch each coding agent in a pane. Each is fed into
/// the pane's shell once it's spawned at final size, and when the agent exits the
/// pane drops back to a normal shell. Edit these to change the model, effort, or
/// permission flags without rebuilding.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Agents {
    pub claude: String,
    pub codex: String,
    pub hermes: String,
}

impl Default for Agents {
    fn default() -> Self {
        Agents {
            claude: "claude --effort max --dangerously-skip-permissions".into(),
            // Run Codex unattended via config overrides rather than the
            // `--dangerously-bypass-approvals-and-sandbox` flag: that flag forces
            // the dangerous mode and makes Codex show a "continue?" confirmation on
            // every launch, whereas the same settings applied through `-c` run with
            // no prompt at all.
            codex: "codex -c model_reasoning_effort=xhigh -c approval_policy=never \
                    -c sandbox_mode=danger-full-access"
                .into(),
            hermes: "hermes chat --yolo".into(),
        }
    }
}

impl Agents {
    /// The configured command for `agent`.
    pub fn command_for(&self, agent: Agent) -> &str {
        match agent {
            Agent::Claude => &self.claude,
            Agent::Codex => &self.codex,
            Agent::Hermes => &self.hermes,
        }
    }
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
            agents: Agents::default(),
            coordination: Coordination::default(),
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
                eprintln!("tcode: config parse error ({e}); using defaults");
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
        let path = config_path();
        // Don't silently clobber a config file that currently fails to parse: a single
        // typo would otherwise be lost the moment the app — now running on defaults
        // (see from_str_or_default) — next persists (e.g. a zoom/font change). Back the
        // unparseable file up to <name>.bak first so the user can recover their values.
        if let Ok(existing) = std::fs::read_to_string(&path) {
            if toml::from_str::<Config>(&existing).is_err() {
                let mut bak = path.clone().into_os_string();
                bak.push(".bak");
                let _ = std::fs::rename(&path, PathBuf::from(bak));
            }
        }
        let text = match toml::to_string_pretty(self) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("tcode: failed to serialize config: {e}");
                return;
            }
        };
        // Atomic + owner-only (0o600): a torn write must never leave a half-file
        // that parses back to defaults and silently loses the user's settings.
        if let Err(e) = crate::fsutil::atomic_write(&path, text.as_bytes(), 0o600) {
            eprintln!("tcode: failed to write config: {e}");
        }
    }
}

/// Tcode's base config directory: `$XDG_CONFIG_HOME/tcode` or
/// `~/.config/tcode`. Shared by the config file and saved sessions.
pub fn config_dir() -> PathBuf {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
    )
}

/// Resolve the config dir from explicit env values (kept separate so it's testable
/// without mutating the process environment). Per the XDG spec a non-absolute
/// (including empty) `XDG_CONFIG_HOME` is invalid and ignored; likewise a missing or
/// relative `HOME` must never yield a CWD-relative path — that would read and write
/// config + sessions wherever tcode happened to be launched, silently hiding the
/// real ones. Only in that pathological case do we fall back to an absolute temp base.
fn config_dir_from(xdg: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> PathBuf {
    if let Some(xdg) = xdg.map(PathBuf::from).filter(|p| p.is_absolute()) {
        return xdg.join("tcode");
    }
    if let Some(home) = home.map(PathBuf::from).filter(|p| p.is_absolute()) {
        return home.join(".config").join("tcode");
    }
    std::env::temp_dir().join("tcode")
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
    fn agent_commands_default_to_bypass_mode() {
        let a = Agents::default();
        assert!(a
            .command_for(Agent::Claude)
            .contains("--dangerously-skip-permissions"));
        // Codex is unattended via config overrides — and must NOT use the forcing
        // flag, which would re-introduce the per-launch "continue?" confirmation.
        let codex = a.command_for(Agent::Codex);
        assert!(codex.contains("approval_policy=never"));
        assert!(codex.contains("sandbox_mode=danger-full-access"));
        assert!(!codex.contains("--dangerously-bypass"));
        assert!(a.command_for(Agent::Hermes).contains("--yolo"));
    }

    #[test]
    fn agents_section_merges_from_partial_toml() {
        // A user overriding just one agent keeps defaults for the others.
        let c = Config::from_str_or_default("[agents]\nclaude = \"claude\"");
        assert_eq!(c.agents.claude, "claude");
        assert!(c.agents.codex.contains("codex"));
        assert!(c.agents.hermes.contains("hermes"));
    }

    #[test]
    fn partial_theme_merges() {
        let c = Config::from_str_or_default("[theme]\naccent = \"#ff0000\"");
        assert_eq!(c.theme.accent, "#ff0000");
        assert_eq!(c.theme.background, "#1a1b26");
    }

    #[test]
    fn config_dir_honors_absolute_xdg() {
        let d = config_dir_from(Some("/xdg".into()), Some("/home/u".into()));
        assert_eq!(d, PathBuf::from("/xdg/tcode"));
    }

    #[test]
    fn config_dir_ignores_empty_or_relative_xdg() {
        // Empty XDG_CONFIG_HOME → fall back to HOME/.config (NOT "tcode" under CWD).
        let d = config_dir_from(Some("".into()), Some("/home/u".into()));
        assert_eq!(d, PathBuf::from("/home/u/.config/tcode"));
        // A relative XDG value is likewise invalid and ignored.
        let d = config_dir_from(Some("rel/path".into()), Some("/home/u".into()));
        assert_eq!(d, PathBuf::from("/home/u/.config/tcode"));
    }

    #[test]
    fn config_dir_never_relative_without_home() {
        // No usable HOME/XDG: the result must still be absolute, never CWD-relative.
        assert!(config_dir_from(Some("".into()), None).is_absolute());
        assert!(config_dir_from(None, Some("relative-home".into())).is_absolute());
    }
}
