//! A single terminal pane: a VTE terminal in a styled container, spawning the
//! user's shell over a PTY. Lifecycle (focus tracking, exit-removal) is wired by
//! the grid, which owns the panes — each pane carries a stable `id` for that.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

use gtk4::gdk::{ModifierType, BUTTON_PRIMARY, BUTTON_SECONDARY, RGBA};
use gtk4::glib::SpawnFlags;
use gtk4::pango::FontDescription;
use gtk4::prelude::*;
use gtk4::{
    gio, Box as GtkBox, EventSequenceState, GestureClick, Orientation, Overlay, PopoverMenu,
    PropagationPhase,
};
use tessera_core::config::Config;
use vte4::prelude::*;
use vte4::{Format, PtyFlags, Regex, Terminal};

use crate::theme::rgba;

/// Open a resolved filesystem path in the editor/viewer panel. Supplied by the
/// app so a Ctrl+clicked path in the terminal lands in the inline viewer.
pub type OpenFn = Rc<dyn Fn(&Path)>;

// PCRE2 compile flags for VTE match regexes (mirrors gnome-terminal's defaults).
const PCRE2_CASELESS: u32 = 0x0000_0008;
const PCRE2_MULTILINE: u32 = 0x0000_0400;
const PCRE2_UCP: u32 = 0x0002_0000;
const PCRE2_UTF: u32 = 0x0008_0000;
const MATCH_FLAGS: u32 = PCRE2_UTF | PCRE2_UCP | PCRE2_MULTILINE | PCRE2_CASELESS;

// Clickable patterns: web/file URLs, then filesystem paths (absolute / ~ / ./),
// then relative paths that include a directory, then bare filenames with a known
// asset extension.
const URL_RE: &str = r#"(?:https?|ftp|file)://[^\s<>"'`]+"#;
const PATH_RE: &str = r#"(?:~|\.{1,2})?/[^\s<>"'`:]+"#;
// Relative paths with a directory component, e.g. `src/main.rs` or
// `crates/tessera/src/app.rs:12:5` (rustc/grep output) — neither PATH_RE (needs a
// leading /, ~/, ./, ..) nor FILE_RE (rejects /) catches these. The lookbehind
// stops it matching inside an absolute path; the :line[:col] suffix is stripped
// in open_link before the existence check.
const REL_PATH_RE: &str = r#"(?<![/\w.~-])[\w.\-]+(?:/[\w.\-]+)+(?::\d+){0,2}"#;
const FILE_RE: &str = r#"[^\s<>"'`:/]+\.(?:png|jpe?g|gif|webp|bmp|svg|ico|tiff?|pdf|docx?|pptx?|xlsx?|odt|odp|ods|mp4|webm|mkv|mov|avi|mp3|wav|flac|ogg|opus|txt|md|markdown|json|toml|ya?ml|rs|jsx?|tsx?|c|h|hpp|cpp|go|rb|java|sh|html?|css|csv|log|conf|ini)"#;

/// Scrollback kept during normal use; dropped to 0 only while a divider is being
/// dragged or a pane is zooming (see `set_resizing`). VTE fills a growing pane by
/// pulling scrollback lines *above* the prompt, burying it — dropping scrollback
/// just for the size change keeps the prompt at the top while retaining history.
const SCROLLBACK_LINES: i64 = 10_000;

pub struct Pane {
    /// Stable identity, assigned by the grid (survives re-tiling/reordering).
    pub id: u64,
    /// Styled root that goes into the grid (carries the `.pane` CSS class).
    pub root: Overlay,
    pub terminal: Terminal,
    /// Focus-ring overlay child (detached around re-parenting — see `detach_ring`).
    ring: GtkBox,
}

impl Pane {
    pub fn new(cfg: &Config, id: u64, on_open: OpenFn) -> Pane {
        let terminal = Terminal::new();

        // Cell colors come from the VTE API, not CSS.
        let fg = rgba(&cfg.theme.foreground);
        let bg = rgba(&cfg.theme.background);
        // VTE only accepts a palette of 0, 8, 16, 232, or 256 colors; coerce any
        // other length (a mis-sized config) to 16 by cycling, so a bad config
        // can't trip a GLib g_return_if_fail critical / abort.
        let palette: Vec<RGBA> = cfg.theme.palette.iter().map(|c| rgba(c)).collect();
        let palette: Vec<RGBA> = match palette.len() {
            0 | 8 | 16 | 232 | 256 => palette,
            n => (0..16).map(|i| palette[i % n]).collect(),
        };
        let palette_refs: Vec<&RGBA> = palette.iter().collect();
        terminal.set_colors(Some(&fg), Some(&bg), &palette_refs);

        let fd = FontDescription::from_string(&format!("{} {}", cfg.font, cfg.font_size));
        terminal.set_font(Some(&fd));
        terminal.set_scrollback_lines(SCROLLBACK_LINES);

        // Ctrl+click a URL or file path to follow it (VS Code-style).
        install_links(&terminal, on_open);
        // Right-click context menu (Copy / Paste / Select All).
        install_context_menu(&terminal);

        let root = Overlay::new();
        root.add_css_class("pane");
        root.set_child(Some(&terminal));

        // Focus ring: an overlay child drawn on top of the terminal. Because it's
        // an overlay it adds no layout space and never reflows the terminal; CSS
        // turns its border cyan only when the pane is active.
        let ring = GtkBox::new(Orientation::Horizontal, 0);
        ring.add_css_class("focus-ring");
        ring.set_can_target(false); // clicks pass through to the terminal
        root.add_overlay(&ring);

        let pane = Pane {
            id,
            root,
            terminal,
            ring,
        };
        pane.spawn(cfg);
        pane
    }

    /// Remove the focus-ring overlay child. The grid rebuilds by unparenting each
    /// pane; a GtkOverlay carrying an overlay child re-parents to a *blank* GL
    /// surface on some drivers (e.g. zoom showed an empty pane). Detaching the
    /// ring first, then re-attaching after the move, keeps the terminal painted.
    pub fn detach_ring(&self) {
        self.root.remove_overlay(&self.ring);
    }

    /// Re-attach the focus ring after the pane has been re-parented.
    pub fn attach_ring(&self) {
        self.root.add_overlay(&self.ring);
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
            vec![
                shell.clone(),
                "-c".into(),
                format!("{startup}; exec {shell}"),
            ]
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

    /// Copy the current selection to the clipboard (no-op without a selection,
    /// so it never clobbers the clipboard).
    pub fn copy(&self) {
        if self.terminal.has_selection() {
            self.terminal.copy_clipboard_format(Format::Text);
        }
    }

    /// Paste the clipboard into the terminal's child.
    pub fn paste(&self) {
        self.terminal.paste_clipboard();
    }

    /// Drop scrollback to 0 while a divider is being dragged so VTE can't pull
    /// stale/blank lines above the prompt as a pane grows (the prompt otherwise
    /// jumps to the bottom). Restore the normal scrollback once the drag settles.
    pub fn set_resizing(&self, on: bool) {
        self.terminal
            .set_scrollback_lines(if on { 0 } else { SCROLLBACK_LINES });
    }

    pub fn set_active(&self, active: bool) {
        if active {
            self.root.add_css_class("active-pane");
        } else {
            self.root.remove_css_class("active-pane");
        }
    }
}

/// Register clickable regexes + OSC 8 hyperlinks, and a Ctrl+click handler that
/// opens web URLs in the browser and file paths in the inline viewer.
fn install_links(terminal: &Terminal, on_open: OpenFn) {
    terminal.set_allow_hyperlink(true);
    for pat in [URL_RE, PATH_RE, REL_PATH_RE, FILE_RE] {
        if let Ok(re) = Regex::for_match(pat, MATCH_FLAGS) {
            let tag = terminal.match_add_regex(&re, 0);
            terminal.match_set_cursor_name(tag, "pointer");
        }
    }

    let click = GestureClick::new();
    click.set_button(BUTTON_PRIMARY);
    // Capture phase so we can claim a Ctrl+click before VTE starts a selection.
    click.set_propagation_phase(PropagationPhase::Capture);
    // Weak: the terminal owns this controller, so a strong capture here would
    // form a cycle and leak the terminal (+ its PTY) when the pane closes.
    let term = terminal.downgrade();
    click.connect_pressed(move |g, _n, x, y| {
        if !g.current_event_state().contains(ModifierType::CONTROL_MASK) {
            return;
        }
        let Some(term) = term.upgrade() else { return };
        let hit = term
            .check_hyperlink_at(x, y)
            .or_else(|| term.check_match_at(x, y).0);
        if let Some(s) = hit {
            if open_link(&s, &term, &on_open) {
                g.set_state(EventSequenceState::Claimed);
            }
        }
    });
    terminal.add_controller(click);
}

/// Route a matched string: web URLs to the browser, file paths/URIs to the
/// inline viewer. Returns whether it was handled.
fn open_link(matched: &str, terminal: &Terminal, on_open: &OpenFn) -> bool {
    let s = matched.trim().trim_end_matches(|c: char| {
        matches!(
            c,
            '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}' | '\'' | '"' | '>'
        )
    });
    if s.is_empty() {
        return false;
    }
    if s.starts_with("http://") || s.starts_with("https://") || s.starts_with("ftp://") {
        let _ = Command::new("xdg-open").arg(s).spawn();
        return true;
    }
    // Drop a trailing :line[:col] (rustc/grep style) before resolving the path.
    let s = strip_line_col(s);
    let path: Option<PathBuf> = if s.starts_with("file://") {
        gio::File::for_uri(s).path()
    } else if let Some(rest) = s.strip_prefix("~/") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(rest))
    } else if s.starts_with('/') {
        Some(PathBuf::from(s))
    } else {
        Some(term_cwd(terminal).join(s))
    };
    match path {
        Some(p) if p.exists() => {
            on_open(&p);
            true
        }
        _ => false,
    }
}

/// Strip a trailing `:line` or `:line:col` suffix (compiler/grep style) so the
/// remaining filesystem path can be checked for existence.
fn strip_line_col(s: &str) -> &str {
    let mut s = s;
    for _ in 0..2 {
        match s.rsplit_once(':') {
            Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => {
                s = head;
            }
            _ => break,
        }
    }
    s
}

/// The terminal's working directory (OSC 7), falling back to the process cwd.
fn term_cwd(terminal: &Terminal) -> PathBuf {
    terminal
        .current_directory_uri()
        .and_then(|uri| gio::File::for_uri(&uri).path())
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")))
}

/// Right-click context menu: Copy / Paste / Select All — the standard terminal
/// mouse affordance VTE doesn't provide itself. (Selection-drag, scroll, and
/// middle-click paste of the primary selection are handled by VTE directly.)
fn install_context_menu(terminal: &Terminal) {
    let menu = gio::Menu::new();
    menu.append(Some("Copy"), Some("term.copy"));
    menu.append(Some("Paste"), Some("term.paste"));
    menu.append(Some("Select All"), Some("term.select-all"));

    let popover = PopoverMenu::from_model(Some(&menu));
    popover.set_parent(terminal);
    popover.set_has_arrow(false);

    let actions = gio::SimpleActionGroup::new();

    // Weak terminal refs throughout: the action group + the gesture below are
    // owned by the terminal, so strong captures would cycle and leak it.
    let copy = gio::SimpleAction::new("copy", None);
    {
        let t = terminal.downgrade();
        copy.connect_activate(move |_, _| {
            if let Some(t) = t.upgrade() {
                if t.has_selection() {
                    t.copy_clipboard_format(Format::Text);
                }
            }
        });
    }
    actions.add_action(&copy);

    let paste = gio::SimpleAction::new("paste", None);
    {
        let t = terminal.downgrade();
        paste.connect_activate(move |_, _| {
            if let Some(t) = t.upgrade() {
                t.paste_clipboard();
            }
        });
    }
    actions.add_action(&paste);

    let select_all = gio::SimpleAction::new("select-all", None);
    {
        let t = terminal.downgrade();
        select_all.connect_activate(move |_, _| {
            if let Some(t) = t.upgrade() {
                t.select_all();
            }
        });
    }
    actions.add_action(&select_all);

    terminal.insert_action_group("term", Some(&actions));

    // Right-click pops the menu at the pointer. Capture phase + claim so it shows
    // reliably instead of VTE's own right-button handling.
    let click = GestureClick::new();
    click.set_button(BUTTON_SECONDARY);
    click.set_propagation_phase(PropagationPhase::Capture);
    {
        let (t, pop) = (terminal.downgrade(), popover.clone());
        click.connect_pressed(move |g, _n, x, y| {
            let Some(t) = t.upgrade() else { return };
            copy.set_enabled(t.has_selection());
            pop.set_pointing_to(Some(&gtk4::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            pop.popup();
            g.set_state(EventSequenceState::Claimed);
        });
    }
    terminal.add_controller(click);
}
