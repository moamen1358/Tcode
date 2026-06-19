//! Application state and window wiring: borderless window, picker ↔ (sidebar + grid),
//! and the image drag-and-drop target.

use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Button, HeaderBar, Orientation, Paned, ToggleButton};
use loom_core::config::Config;
use loom_core::session::{self, Session};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use crate::editor::Editor;
use crate::grid::Grid;
use crate::sidebar::Sidebar;
use crate::{keys, session_picker, theme};

pub struct State {
    pub window: ApplicationWindow,
    pub cfg: Config,
    pub grid: Option<Grid>,
    pub sidebar: Option<Sidebar>,
    pub sidebar_btn: ToggleButton,
    pub editor: Option<Editor>,
    pub editor_btn: ToggleButton,
    pub shots_btn: ToggleButton,
    pub shots_panel: Option<gtk4::Box>,
    /// Frame capture action (region-select → annotate), set once the grid
    /// is built; the titlebar camera button invokes it.
    pub shots_capture: Option<Rc<dyn Fn()>>,
    /// Clipboard-history panel — built once and re-parented across relayouts so
    /// the history and its single clipboard watcher persist.
    pub clipboard: Option<Rc<crate::clipboard::Panel>>,
    /// The session currently open in this window, if any.
    pub current: Option<Session>,
    /// Whether to persist `current` on changes. False for `loom N` quick
    /// launches (ephemeral), true for sessions opened/created via the picker.
    pub save_sessions: bool,
    /// Titlebar session switcher: shows the current name, popover lists/creates.
    pub session_btn: gtk4::MenuButton,
    /// Readouts in the view-settings popover, kept in sync by font/scale changes.
    pub font_readout: gtk4::Label,
    pub scale_readout: gtk4::Label,
    /// Center split (terminals | editor) and outer split (sidebar | rest), kept so
    /// their drag positions can be saved into the session and restored.
    pub center_paned: Option<Paned>,
    pub content_paned: Option<Paned>,
}

pub type Shared = Rc<RefCell<State>>;

pub fn build(app: &Application, preset: Option<usize>) {
    let cfg = Config::load();
    theme::install_css(&cfg.theme, &cfg.font, cfg.font_size, cfg.scale);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Loom")
        .default_width(1280)
        .default_height(800)
        .maximized(true)
        .build();

    // Minimal VS Code-style titlebar (client-side decorations): a thin bar that
    // carries the minimize / maximize / close buttons, so the window can always
    // be closed or restored. Press Alt+f for an immersive fullscreen (no header).
    let header = HeaderBar::new();
    header.add_css_class("loom-titlebar");

    // Clickable sidebar toggle (also bound to Alt+b), VS Code-style — the
    // Adwaita "show sidebar" icon (a panel with a highlighted left bar).
    let sidebar_btn = ToggleButton::new();
    sidebar_btn.set_icon_name("sidebar-show-symbolic");
    sidebar_btn.set_active(true);
    sidebar_btn.set_tooltip_text(Some("Toggle file panel (Alt+B)"));
    sidebar_btn.add_css_class("flat");
    header.pack_start(&sidebar_btn);

    // Editor (right panel) toggle — show/hide the file editor.
    let editor_btn = ToggleButton::new();
    editor_btn.set_icon_name("sidebar-show-right-symbolic");
    editor_btn.set_active(true);
    editor_btn.set_tooltip_text(Some("Toggle file editor"));
    editor_btn.add_css_class("flat");
    header.pack_end(&editor_btn);

    // Screenshots section toggle (also bound to Alt+P) — show/hide the gallery
    // strip at the bottom of the file sidebar. Visible by default; since it now
    // lives in the sidebar it doesn't steal width from the editor/viewer.
    let shots_btn = ToggleButton::new();
    shots_btn.set_icon_name("image-x-generic-symbolic");
    shots_btn.set_active(true);
    shots_btn.set_tooltip_text(Some("Toggle screenshots strip (Alt+P)"));
    shots_btn.add_css_class("flat");
    header.pack_end(&shots_btn);

    // Frame capture: region-select a screenshot, annotate, save to the panel.
    let capture_btn = Button::from_icon_name("camera-photo-symbolic");
    capture_btn.set_tooltip_text(Some("Capture a screenshot"));
    capture_btn.add_css_class("flat");
    header.pack_end(&capture_btn);

    // View settings (font size + whole-UI scale) — a titlebar popover.
    let font_readout = gtk4::Label::new(None);
    let scale_readout = gtk4::Label::new(None);
    let view_btn = gtk4::MenuButton::new();
    view_btn.set_icon_name("preferences-system-symbolic");
    view_btn.set_tooltip_text(Some("Font size & scale"));
    view_btn.add_css_class("flat");
    header.pack_end(&view_btn);

    // Centered session switcher: shows the current session's name; its popover
    // lists saved sessions (click to switch) and a New-session action.
    let session_btn = gtk4::MenuButton::new();
    session_btn.set_label("Loom");
    session_btn.add_css_class("session-switcher");
    session_btn.set_tooltip_text(Some("Switch session"));
    header.set_title_widget(Some(&session_btn));

    window.set_titlebar(Some(&header));

    let state: Shared = Rc::new(RefCell::new(State {
        window: window.clone(),
        cfg,
        grid: None,
        sidebar: None,
        sidebar_btn: sidebar_btn.clone(),
        editor: None,
        editor_btn: editor_btn.clone(),
        shots_btn: shots_btn.clone(),
        shots_panel: None,
        shots_capture: None,
        clipboard: None,
        current: None,
        save_sessions: false,
        session_btn: session_btn.clone(),
        font_readout: font_readout.clone(),
        scale_readout: scale_readout.clone(),
        center_paned: None,
        content_paned: None,
    }));

    // Build the view-settings popover: font-size + scale steppers + reset.
    {
        let popover = gtk4::Popover::new();
        popover.add_css_class("session-popover");
        let col = gtk4::Box::new(Orientation::Vertical, 6);
        col.set_margin_top(8);
        col.set_margin_bottom(8);
        col.set_margin_start(8);
        col.set_margin_end(8);
        col.append(&view_row("Font size", &font_readout, {
            let st = state.clone();
            move |step| change_font_size(&st, step)
        }));
        col.append(&view_row("Scale", &scale_readout, {
            let st = state.clone();
            move |step| change_scale(&st, step)
        }));
        let reset = Button::with_label("Reset scale");
        reset.add_css_class("session-menu-row");
        {
            let st = state.clone();
            reset.connect_clicked(move |_| reset_view(&st));
        }
        col.append(&reset);
        popover.set_child(Some(&col));
        view_btn.set_popover(Some(&popover));
    }

    // Flip the current sidebar's visibility whenever the button toggles.
    {
        let st = state.clone();
        sidebar_btn.connect_toggled(move |btn| {
            if let Some(sb) = st.borrow().sidebar.as_ref() {
                sb.root.set_visible(btn.is_active());
            }
        });
    }

    // Show/hide the file editor panel.
    {
        let st = state.clone();
        editor_btn.connect_toggled(move |btn| {
            if let Some(ed) = st.borrow().editor.as_ref() {
                ed.root.set_visible(btn.is_active());
            }
        });
    }

    // Show/hide the left screenshots panel.
    {
        let st = state.clone();
        shots_btn.connect_toggled(move |btn| {
            if let Some(p) = st.borrow().shots_panel.as_ref() {
                p.set_visible(btn.is_active());
            }
        });
    }

    // Camera button: reveal the screenshots panel, then start a capture
    // (region-select → annotate → save lands in the panel).
    {
        let st = state.clone();
        capture_btn.connect_clicked(move |_| {
            let cap = {
                let s = st.borrow();
                s.shots_btn.set_active(true); // reveal the panel so the result shows
                s.shots_capture.clone()
            };
            if let Some(cap) = cap {
                cap();
            }
        });
    }

    // "+" button next to the sidebar toggle: add a new terminal pane (also Alt+n).
    let add_btn = Button::from_icon_name("list-add-symbolic");
    add_btn.set_tooltip_text(Some("New terminal (Alt+N)"));
    add_btn.add_css_class("flat");
    header.pack_start(&add_btn);
    {
        let st = state.clone();
        add_btn.connect_clicked(move |_| {
            if let Some(g) = st.borrow().grid.as_ref() {
                g.add_pane();
            }
        });
    }

    keys::install(&window, &state);

    // Save the open session when the window closes.
    {
        let st = state.clone();
        window.connect_close_request(move |_| {
            save_current(&st);
            gtk4::glib::Propagation::Proceed
        });
    }

    match preset {
        // `loom N`: a quick ephemeral session (N panes, current dir), no picker.
        Some(n) => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let mut s = Session::new(cwd);
            s.panes = n;
            open_session(&state, s);
        }
        None => show_session_picker(&state),
    }
    refresh_session_menu(&state);
    apply_view(&state);
    refresh_view_readout(&state);
    window.present();
}

/// A "title  [−] readout [+]" stepper row for the view popover. `on_step` is
/// called with -1 / +1.
fn view_row(title: &str, readout: &gtk4::Label, on_step: impl Fn(i32) + 'static) -> gtk4::Box {
    let on_step = Rc::new(on_step);
    let row = gtk4::Box::new(Orientation::Horizontal, 8);
    let t = gtk4::Label::new(Some(title));
    t.set_xalign(0.0);
    t.set_hexpand(true);
    let minus = Button::with_label("\u{2212}");
    minus.add_css_class("view-step");
    let plus = Button::with_label("+");
    plus.add_css_class("view-step");
    readout.set_width_chars(5);
    readout.add_css_class("view-readout");
    {
        let f = on_step.clone();
        minus.connect_clicked(move |_| f(-1));
    }
    {
        let f = on_step.clone();
        plus.connect_clicked(move |_| f(1));
    }
    row.append(&t);
    row.append(&minus);
    row.append(readout);
    row.append(&plus);
    row
}

/// Apply the current font size + UI scale: scale all widget fonts via the font
/// DPI, set each terminal's base font and zoom, and refresh the editor CSS.
pub fn apply_view(state: &Shared) {
    let (font, size, scale) = {
        let s = state.borrow();
        (s.cfg.font.clone(), s.cfg.font_size, s.cfg.scale)
    };
    // Chrome (sidebar, editor, picker, clipboard, …) scales via the stylesheet;
    // terminals scale via VTE's font-scale. Both use the same factor.
    {
        let s = state.borrow();
        theme::install_css(&s.cfg.theme, &s.cfg.font, s.cfg.font_size, s.cfg.scale);
    }
    if let Some(g) = state.borrow().grid.as_ref() {
        g.apply_font(&font, size, scale);
    }
}

/// Sync the popover readouts with the current font size / scale.
fn refresh_view_readout(state: &Shared) {
    let s = state.borrow();
    s.font_readout.set_text(&format!("{} pt", s.cfg.font_size));
    s.scale_readout
        .set_text(&format!("{}%", (s.cfg.scale * 100.0).round() as i32));
}

/// Step the base font size, apply, and persist.
pub fn change_font_size(state: &Shared, step: i32) {
    {
        let mut s = state.borrow_mut();
        s.cfg.font_size = (s.cfg.font_size as i32 + step).clamp(4, 96) as u32;
    }
    apply_view(state);
    refresh_view_readout(state);
    state.borrow().cfg.save();
}

/// Step the UI scale by 10% per step, apply, and persist.
pub fn change_scale(state: &Shared, step: i32) {
    {
        let mut s = state.borrow_mut();
        let new = s.cfg.scale + step as f64 * 0.1;
        s.cfg.scale = ((new * 10.0).round() / 10.0).clamp(Config::MIN_SCALE, Config::MAX_SCALE);
    }
    apply_view(state);
    refresh_view_readout(state);
    state.borrow().cfg.save();
}

/// Reset the UI scale to 100% (leaves the font size as-is).
pub fn reset_view(state: &Shared) {
    state.borrow_mut().cfg.scale = 1.0;
    apply_view(state);
    refresh_view_readout(state);
    state.borrow().cfg.save();
}

/// Show the startup session picker (resume a saved session or start a new one).
pub fn show_session_picker(state: &Shared) {
    let st_open = state.clone();
    let st_new = state.clone();
    let widget = session_picker::build(
        session::list(),
        move |s| {
            st_open.borrow_mut().save_sessions = true;
            open_session(&st_open, s);
            refresh_session_menu(&st_open);
        },
        move || new_session(&st_new),
    );
    state.borrow().window.set_child(Some(&widget));
}

/// Show the "new session" screen: choose a folder + terminal count, then create
/// and open it. Cancelling returns to the previous session (or the picker).
fn new_session(state: &Shared) {
    // Persist the outgoing session before leaving it, so creating a new session
    // from inside one doesn't discard its unsaved files/pane changes.
    save_current(state);
    let window = state.borrow().window.clone();
    let prev = state.borrow().current.clone();
    let st_create = state.clone();
    let st_cancel = state.clone();
    let widget = session_picker::build_new(
        window,
        move |folder, panes| {
            let mut s = Session::new(folder);
            s.panes = panes;
            s.save();
            st_create.borrow_mut().save_sessions = true;
            open_session(&st_create, s);
        },
        move || match prev.clone() {
            Some(s) => open_session(&st_cancel, s),
            None => show_session_picker(&st_cancel),
        },
    );
    state.borrow().window.set_child(Some(&widget));
}

/// Make `session` the current one: chdir to its root, build the grid with its
/// pane count, then reopen its files.
pub fn open_session(state: &Shared, session: Session) {
    let root = session.root.clone();
    // If the saved root is gone, don't silently open in a stale cwd — send the
    // user back to the picker to pick/recreate instead.
    if !root.is_dir() {
        eprintln!(
            "loom: session root {} is missing; returning to picker",
            root.display()
        );
        show_session_picker(state);
        return;
    }
    let panes = session.panes.max(1);
    let files = session.files.clone();
    let active = session.active;
    let _ = std::env::set_current_dir(&root);
    state.borrow_mut().current = Some(session);

    show_grid(state, panes);

    for f in &files {
        // Skip anything that's gone or is no longer a regular file.
        if f.is_file() {
            open_file(state, f);
        }
    }
    if let Some(idx) = active {
        if let Some(editor) = state.borrow().editor.as_ref() {
            editor.root.set_current_page(Some(idx as u32));
        }
    }
    refresh_session_menu(state);
}

/// Switch to another saved session: persist the current one first.
fn switch_session(state: &Shared, session: Session) {
    // Clicking the already-open session shouldn't tear it down and rebuild it.
    if state
        .borrow()
        .current
        .as_ref()
        .is_some_and(|c| c.id == session.id)
    {
        return;
    }
    save_current(state);
    state.borrow_mut().save_sessions = true;
    open_session(state, session);
}

/// Snapshot the live UI (open files, active tab, pane count, split sizes) into
/// `current`.
fn capture_current(state: &Shared) {
    let (files, active, panes, divisors, editor_split, sidebar_width) = {
        let s = state.borrow();
        if s.current.is_none() {
            return;
        }
        // Editor split only when the editor is actually shown (a file is open).
        let editor_split = s.center_paned.as_ref().and_then(|c| {
            (c.end_child().is_some() && c.width() > 1)
                .then(|| c.position() as f64 / c.width() as f64)
        });
        let sidebar_width = s
            .content_paned
            .as_ref()
            .map(|c| c.position())
            .filter(|&p| p > 0);
        (
            s.editor
                .as_ref()
                .map(|e| e.open_files())
                .unwrap_or_default(),
            s.editor.as_ref().and_then(|e| e.active_index()),
            s.grid.as_ref().map(|g| g.pane_count()).unwrap_or(1),
            s.grid
                .as_ref()
                .map(|g| g.split_ratios())
                .unwrap_or_default(),
            editor_split,
            sidebar_width,
        )
    };
    if let Some(cur) = state.borrow_mut().current.as_mut() {
        cur.files = files;
        cur.active = active;
        cur.panes = panes;
        cur.divisors = divisors;
        cur.editor_split = editor_split;
        cur.sidebar_width = sidebar_width;
    }
}

/// Capture + persist the current session (only if it's a saved one).
pub fn save_current(state: &Shared) {
    capture_current(state);
    let s = state.borrow();
    if s.save_sessions {
        if let Some(cur) = s.current.as_ref() {
            cur.save();
        }
    }
}

/// Rebuild the terminal grid with `n` panes, keeping the session's open files.
pub fn set_panes(state: &Shared, n: usize) {
    // Ignore Alt+digit when there's no active session/grid (e.g. on the picker),
    // so it can't spawn an orphan, session-less grid.
    if state.borrow().current.is_none() {
        return;
    }
    capture_current(state);
    let session = {
        let mut s = state.borrow_mut();
        if let Some(cur) = s.current.as_mut() {
            cur.panes = n;
            cur.divisors.clear(); // count changed → reset to equal splits
        }
        s.current.clone()
    };
    if let Some(s) = session {
        open_session(state, s);
    }
}

/// Update the titlebar switcher: current name as label, popover with the session
/// list (click to switch) plus a New-session action.
fn refresh_session_menu(state: &Shared) {
    let btn = state.borrow().session_btn.clone();
    let name = state
        .borrow()
        .current
        .as_ref()
        .map(|c| c.name.clone())
        .unwrap_or_else(|| "Loom".to_string());
    btn.set_label(&name);

    let current_id = state.borrow().current.as_ref().map(|c| c.id.clone());
    let popover = gtk4::Popover::new();
    popover.add_css_class("session-popover");
    let col = gtk4::Box::new(Orientation::Vertical, 2);
    col.set_margin_top(6);
    col.set_margin_bottom(6);
    col.set_margin_start(6);
    col.set_margin_end(6);
    col.add_css_class("session-menu");

    for sess in session::list() {
        let row = Button::with_label(&sess.name);
        row.add_css_class("session-menu-row");
        if Some(&sess.id) == current_id.as_ref() {
            row.add_css_class("current");
        }
        let st = state.clone();
        let pop = popover.downgrade();
        btn_on_click_switch(
            &row,
            move |s| {
                if let Some(p) = pop.upgrade() {
                    p.popdown();
                }
                switch_session(&st, s);
            },
            sess,
        );
        col.append(&row);
    }

    col.append(&gtk4::Separator::new(Orientation::Horizontal));
    let new = Button::with_label("＋  New session");
    new.add_css_class("session-menu-new");
    {
        let st = state.clone();
        let pop = popover.downgrade();
        new.connect_clicked(move |_| {
            if let Some(p) = pop.upgrade() {
                p.popdown();
            }
            new_session(&st);
        });
    }
    col.append(&new);

    popover.set_child(Some(&col));
    btn.set_popover(Some(&popover));
}

/// Wire a session row button to call `f(session)` on click.
fn btn_on_click_switch(row: &Button, f: impl Fn(Session) + 'static, session: Session) {
    row.connect_clicked(move |_| f(session.clone()));
}

pub fn show_grid(state: &Shared, n: usize) {
    // Clone config + window so we don't hold a borrow while Pane::new spawns PTYs.
    let (cfg, window) = {
        let s = state.borrow();
        (s.cfg.clone(), s.window.clone())
    };
    // A Ctrl+clicked path in any terminal opens in the editor/viewer. Weak ref so
    // the grid (held by the state) doesn't form a cycle back to the state.
    let on_open: crate::pane::OpenFn = {
        let weak = Rc::downgrade(state);
        Rc::new(move |path: &std::path::Path| {
            if let Some(s) = weak.upgrade() {
                open_file(&s, path);
            }
        })
    };
    let grid = Grid::new(n, &cfg, &window, on_open);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));

    install_image_drop(&grid.root, state);
    grid.root.set_hexpand(true);
    grid.root.set_vexpand(true);

    // Center: resizable split — terminals on the left, the file editor (revealed
    // when a file is opened from the sidebar) on the right.
    let center = Paned::new(Orientation::Horizontal);
    center.set_hexpand(true);
    center.set_vexpand(true);
    center.set_start_child(Some(&grid.root));
    center.set_resize_start_child(true);
    center.set_shrink_start_child(false);
    center.set_resize_end_child(true);
    center.set_shrink_end_child(false);
    let editor = Editor::new(&center, &cfg.theme.surface);

    // Clicking anywhere in the editor/viewer panel pulls keyboard focus off the
    // terminals, so the active-pane yellow ring clears when you click an image or
    // document (the image canvas alone doesn't grab focus on click). Capture phase
    // runs before the viewer's own gestures; it never claims, so the viewer's
    // pan/zoom still work.
    {
        let nb = editor.root.clone();
        let click = gtk4::GestureClick::new();
        click.set_propagation_phase(gtk4::PropagationPhase::Capture);
        click.connect_pressed(move |_g, _n, _x, _y| {
            if let Some(w) = nb.current_page().and_then(|p| nb.nth_page(Some(p))) {
                w.grab_focus();
            }
        });
        editor.root.add_controller(click);
    }

    let sidebar = Sidebar::new(&cwd, state);
    // Respect the current toggle state for the freshly built sidebar.
    sidebar
        .root
        .set_visible(state.borrow().sidebar_btn.is_active());

    // Resizable split between the sidebar and the rest (drag to set its width).
    let content = Paned::new(Orientation::Horizontal);
    content.set_start_child(Some(&sidebar.root));
    content.set_end_child(Some(&center));
    content.set_resize_start_child(false);
    content.set_shrink_start_child(false);
    content.set_resize_end_child(true);
    content.set_shrink_end_child(false);
    content.set_position(240);

    // Wrap the content with Frame's annotation layer (shown over the content
    // only while editing a capture), and embed the screenshots gallery at the
    // bottom of the file sidebar.
    let bridge = crate::frame::integrate(&window, &content);

    // Clipboard-history strip, above the screenshots strip. Built once and
    // re-parented across relayouts so its history + single clipboard watcher
    // persist.
    let clip = {
        let mut s = state.borrow_mut();
        if s.clipboard.is_none() {
            let persist = s.cfg.clipboard_persist;
            s.clipboard = Some(crate::clipboard::build(persist));
        }
        s.clipboard.clone().unwrap()
    };
    if clip.root.parent().is_some() {
        clip.root.unparent();
    }
    sidebar.root.append(&clip.root);
    sidebar.root.append(&bridge.panel_root);
    window.set_child(Some(&bridge.root));

    {
        let mut s = state.borrow_mut();
        s.grid = Some(grid);
        s.sidebar = Some(sidebar);
        s.editor = Some(editor);
        s.shots_panel = Some(bridge.panel_root.clone());
        s.shots_capture = Some(bridge.capture.clone());
        s.center_paned = Some(center.clone());
        s.content_paned = Some(content.clone());
    }
    // Respect the current toggle state for the freshly built panel.
    bridge
        .panel_root
        .set_visible(state.borrow().shots_btn.is_active());

    // Optionally open a file at startup (LOOM_OPEN=path) — preview/testing aid.
    if let Some(path) = std::env::var_os("LOOM_OPEN") {
        open_file(state, std::path::Path::new(&path));
    }

    // Apply the current font size + UI scale to the freshly built terminals.
    apply_view(state);

    // Once the window is mapped (so panes have a real size), restore the saved
    // split sizes for this session — or fall back to equal splits — and grab
    // keyboard focus (COSMIC drops a focus grabbed before present()).
    let st = state.clone();
    gtk4::glib::idle_add_local_once(move || {
        let s = st.borrow();
        if let Some(g) = s.grid.as_ref() {
            let ratios = s
                .current
                .as_ref()
                .map(|c| c.divisors.clone())
                .unwrap_or_default();
            if ratios.is_empty() {
                g.relayout_positions();
            } else {
                g.apply_split_ratios(&ratios);
            }
            g.grab_focused();
        }
        // Restore the editor split (terminals | editor) and the sidebar width.
        if let (Some(center), Some(ratio)) = (
            s.center_paned.as_ref(),
            s.current.as_ref().and_then(|c| c.editor_split),
        ) {
            let w = center.width();
            if center.end_child().is_some() && w > 1 {
                center.set_position((ratio * w as f64).round() as i32);
            }
        }
        if let (Some(content), Some(sw)) = (
            s.content_paned.as_ref(),
            s.current.as_ref().and_then(|c| c.sidebar_width),
        ) {
            content.set_position(sw);
        }
    });
}

/// Open a file in the editor panel beside the terminals.
pub fn open_file(state: &Shared, path: &std::path::Path) {
    if let Some(editor) = state.borrow().editor.as_ref() {
        editor.open(path);
    }
    // Ensure the editor panel is shown (and the toggle reflects it).
    state.borrow().editor_btn.set_active(true);
}

/// Drag-and-drop image (or any file) → insert its path into the focused pane.
fn install_image_drop(widget: &impl IsA<gtk4::Widget>, state: &Shared) {
    let st = state.clone();
    crate::dnd::install_path_drop(widget, move |paths| {
        let joined = crate::dnd::shell_join_paths(&paths);
        if let Some(g) = st.borrow().grid.as_ref() {
            g.feed_focused(&joined);
        }
    });
}
