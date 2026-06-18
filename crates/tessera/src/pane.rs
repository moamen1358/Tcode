//! A single terminal pane: a VTE terminal in a styled container, spawning the
//! user's shell over a PTY. Lifecycle (focus tracking, exit-removal) is wired by
//! the grid, which owns the panes — each pane carries a stable `id` for that.

use gtk4::gdk::RGBA;
use gtk4::glib::SpawnFlags;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{gio, Overlay};
use tessera_core::config::Config;
use vte4::prelude::*;
use vte4::{PtyFlags, Terminal};

use crate::theme::rgba;

pub struct Pane {
    /// Stable identity, assigned by the grid (survives re-tiling/reordering).
    pub id: u64,
    /// Styled root that goes into the grid (carries the `.pane` CSS class).
    pub root: Overlay,
    pub terminal: Terminal,
}

impl Pane {
    pub fn new(cfg: &Config, id: u64) -> Pane {
        let terminal = Terminal::new();

        // Cell colors come from the VTE API, not CSS.
        let fg = rgba(&cfg.theme.foreground);
        let bg = rgba(&cfg.theme.background);
        let palette: Vec<RGBA> = cfg.theme.palette.iter().map(|c| rgba(c)).collect();
        let palette_refs: Vec<&RGBA> = palette.iter().collect();
        terminal.set_colors(Some(&fg), Some(&bg), &palette_refs);

        let fd = FontDescription::from_string(&format!("{} {}", cfg.font, cfg.font_size));
        terminal.set_font(Some(&fd));

        let root = Overlay::new();
        root.add_css_class("pane");
        root.set_child(Some(&terminal));

        let pane = Pane { id, root, terminal };
        pane.spawn(cfg);
        pane
    }

    /// Spawn the shell (+ optional startup command) over a PTY.
    fn spawn(&self, cfg: &Config) {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
        let cwd = std::env::current_dir()
            .ok()
            .and_then(|p| p.to_str().map(str::to_string))
            .unwrap_or_else(|| "/".into());

        let startup = cfg.startup_command.trim().to_string();
        let argv: Vec<String> = if startup.is_empty() {
            vec![shell.clone(), "-l".into()]
        } else {
            vec![shell.clone(), "-c".into(), format!("{startup}; exec {shell}")]
        };
        let argv_ref: Vec<&str> = argv.iter().map(String::as_str).collect();

        self.terminal.spawn_async(
            PtyFlags::DEFAULT,
            Some(cwd.as_str()),
            &argv_ref,
            &[],
            SpawnFlags::DEFAULT,
            || {},
            -1,
            gio::Cancellable::NONE,
            move |res| {
                if let Err(err) = res {
                    eprintln!("tessera: spawn failed: {err}");
                }
            },
        );
    }

    pub fn grab_focus(&self) {
        self.terminal.grab_focus();
    }

    /// Type text into the terminal's child as if entered at the keyboard.
    pub fn feed_text(&self, text: &str) {
        self.terminal.feed_child(text.as_bytes());
    }

    pub fn set_active(&self, active: bool) {
        if active {
            self.root.add_css_class("active-pane");
        } else {
            self.root.remove_css_class("active-pane");
        }
    }
}
