//! Application state and window wiring: borderless window, picker ↔ (sidebar + grid),
//! and the image drag-and-drop target.

use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Button, HeaderBar, Image, Orientation, Paned, Stack,
    ToggleButton,
};
use tessera_core::config::Config;
use tessera_core::session::{self, Session};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// A session's live content, kept alive (shells still running) while another
/// session is shown, so switching back resumes it instead of rebuilding.
#[derive(Clone)]
struct LiveContent {
    grid: Grid,
    sidebar: Sidebar,
    editor: Editor,
    shots_panel: gtk4::Box,
    shots_capture: Rc<dyn Fn()>,
    center: Paned,
    content: Paned,
}

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
    /// Whether to persist `current` on changes. False for `tessera N` quick
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
    /// Holds one page per open session; switching flips the visible page so each
    /// session's terminals keep running in the background.
    pub stack: Stack,
    /// Live content per session id (kept alive while hidden).
    live: HashMap<String, LiveContent>,
}

pub type Shared = Rc<RefCell<State>>;

pub fn build(app: &Application, preset: Option<usize>) {
    let cfg = Config::load();
    theme::install_css(&cfg.theme, &cfg.font, cfg.font_size, cfg.scale);

    let window = ApplicationWindow::builder()
        .application(app)
        .title("Tessera")
        .default_width(1280)
        .default_height(800)
        .maximized(true)
        .build();

    // Minimal VS Code-style titlebar (client-side decorations): a thin bar that
    // carries the minimize / maximize / close buttons, so the window can always
    // be closed or restored. Press Alt+f for an immersive fullscreen (no header).
    let header = HeaderBar::new();
    header.add_css_class("tessera-titlebar");

    // App logo at the far left of the titlebar.
    let logo = tessera_logo(26);
    logo.set_tooltip_text(Some("Tessera"));
    logo.set_margin_start(8);
    logo.set_margin_end(4);
    logo.set_can_target(false); // decorative — let clicks/drag pass to the bar

    // Clickable sidebar toggle (also bound to Alt+b), VS Code-style — the
    // Adwaita "show sidebar" icon (a panel with a highlighted left bar).
    let sidebar_btn = ToggleButton::new();
    sidebar_btn.set_icon_name("sidebar-show-symbolic");
    sidebar_btn.set_active(true);
    sidebar_btn.set_tooltip_text(Some("Toggle file panel (Alt+B)"));
    sidebar_btn.add_css_class("flat");

    // Editor (right panel) toggle — show/hide the file editor.
    let editor_btn = ToggleButton::new();
    editor_btn.set_icon_name("sidebar-show-right-symbolic");
    editor_btn.set_active(true);
    editor_btn.set_tooltip_text(Some("Toggle file editor"));
    editor_btn.add_css_class("flat");

    // Screenshots section toggle (also bound to Alt+P) — show/hide the gallery
    // strip at the bottom of the file sidebar. Visible by default; since it now
    // lives in the sidebar it doesn't steal width from the editor/viewer.
    let shots_btn = ToggleButton::new();
    shots_btn.set_icon_name("image-x-generic-symbolic");
    shots_btn.set_active(true);
    shots_btn.set_tooltip_text(Some("Toggle screenshots strip (Alt+P)"));
    shots_btn.add_css_class("flat");

    // Frame capture: region-select a screenshot, annotate, save to the panel.
    let capture_btn = Button::from_icon_name("camera-photo-symbolic");
    capture_btn.set_tooltip_text(Some("Capture a screenshot"));
    capture_btn.add_css_class("flat");

    // View settings (font size + whole-UI scale) — a titlebar popover.
    let font_readout = gtk4::Label::new(None);
    let scale_readout = gtk4::Label::new(None);
    let view_btn = gtk4::MenuButton::new();
    view_btn.set_icon_name("preferences-system-symbolic");
    view_btn.set_tooltip_text(Some("Font size & scale"));
    view_btn.add_css_class("flat");

    // "+" new terminal (also Alt+n) — created here so it groups with the actions.
    let add_btn = Button::from_icon_name("list-add-symbolic");
    add_btn.set_tooltip_text(Some("New terminal (Alt+N)"));
    add_btn.add_css_class("flat");

    // Centered session switcher: shows the current session's name; its popover
    // lists saved sessions (click to switch) and a New-session action.
    let session_btn = gtk4::MenuButton::new();
    session_btn.set_label("Tessera");
    session_btn.add_css_class("session-switcher");
    session_btn.set_tooltip_text(Some("Switch session"));

    // Grouped titlebar layout, so the controls read as tidy clusters:
    //   left:   logo · new-terminal · capture      (identity + create actions)
    //   centre: session switcher
    //   right:  [sidebar|editor|shots] · settings   (panel toggles + settings)
    let left_group = gtk4::Box::new(Orientation::Horizontal, 2);
    left_group.append(&logo);
    left_group.append(&add_btn);
    left_group.append(&capture_btn);
    header.pack_start(&left_group);

    // The three panel toggles: flat, evenly spaced, each underlined in orange
    // while its panel is showing.
    sidebar_btn.add_css_class("titlebar-toggle");
    editor_btn.add_css_class("titlebar-toggle");
    shots_btn.add_css_class("titlebar-toggle");
    let right_group = gtk4::Box::new(Orientation::Horizontal, 2);
    right_group.append(&sidebar_btn);
    right_group.append(&editor_btn);
    right_group.append(&shots_btn);
    right_group.append(&view_btn);
    header.pack_end(&right_group);

    header.set_title_widget(Some(&session_btn));
    window.set_titlebar(Some(&header));

    // The window's only child for its lifetime: a stack with one page per open
    // session. Switching flips the visible page so hidden sessions' shells live on.
    let stack = Stack::new();
    stack.set_transition_type(gtk4::StackTransitionType::None);
    window.set_child(Some(&stack));

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
        stack: stack.clone(),
        live: HashMap::new(),
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

    // "+" adds a new terminal pane (the button lives in the titlebar's left group).
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
        // `tessera N`: a quick ephemeral session (N panes, current dir), no picker.
        Some(n) => {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
            let mut s = Session::new(cwd);
            s.panes = n;
            open_session(&state, s);
        }
        // `TESSERA_RESUME=<id>` opens that saved session directly, skipping the
        // picker (handy for scripting a launch straight into a known session).
        None => match std::env::var_os("TESSERA_RESUME")
            .and_then(|id| session::load(&id.to_string_lossy()))
        {
            Some(sess) => {
                state.borrow_mut().save_sessions = true;
                open_session(&state, sess);
            }
            None => show_session_picker(&state),
        },
    }
    refresh_session_menu(&state);
    apply_view(&state);
    refresh_view_readout(&state);
    window.present();
}

/// The Tessera logo as a small titlebar image — the embedded app icon, scaled
/// to `px` device pixels.
fn tessera_logo(px: i32) -> Image {
    let image = Image::new();
    image.set_pixel_size(px);
    let bytes = gtk4::glib::Bytes::from_static(include_bytes!("../assets/tessera.png"));
    let stream = gtk4::gio::MemoryInputStream::from_bytes(&bytes);
    if let Ok(pb) =
        gtk4::gdk_pixbuf::Pixbuf::from_stream_at_scale(&stream, px, px, true, gtk4::gio::Cancellable::NONE)
    {
        image.set_paintable(Some(&gtk4::gdk::Texture::for_pixbuf(&pb)));
    }
    image
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

/// Reserved stack-page name for the transient picker / new-session screens (not
/// a session, so it never collides with a session id and is replaced each time).
const PICKER_PAGE: &str = "__tessera_picker__";

/// Show a transient full-window screen (picker or new-session form) as the
/// reserved stack page, replacing any previous one.
fn show_transient(state: &Shared, widget: &impl IsA<gtk4::Widget>) {
    let (stack, old) = {
        let s = state.borrow();
        (s.stack.clone(), s.stack.child_by_name(PICKER_PAGE))
    };
    if let Some(old) = old {
        stack.remove(&old);
    }
    stack.add_named(widget, Some(PICKER_PAGE));
    stack.set_visible_child_name(PICKER_PAGE);
}

/// Drop the transient picker page once a real session is shown (breaks the
/// picker closures' Rc cycle back to the state).
fn clear_picker_page(state: &Shared) {
    let (stack, child) = {
        let s = state.borrow();
        (s.stack.clone(), s.stack.child_by_name(PICKER_PAGE))
    };
    if let Some(p) = child {
        stack.remove(&p);
    }
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
    show_transient(state, &widget);
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
    show_transient(state, &widget);
}

/// Show `session`: reveal its kept-alive content if it's already open this run,
/// otherwise build it fresh.
pub fn open_session(state: &Shared, session: Session) {
    if !session.root.is_dir() {
        // If the saved root is gone, don't silently open in a stale cwd.
        eprintln!(
            "tessera: session root {} is missing; returning to picker",
            session.root.display()
        );
        show_session_picker(state);
        return;
    }
    if state.borrow().live.contains_key(&session.id) {
        reveal_session(state, session);
    } else {
        build_session(state, session);
    }
}

/// Build a session's content fresh (chdir, grid, files), add it as a stack page,
/// and remember it so switching back later resumes it instead of rebuilding.
fn build_session(state: &Shared, session: Session) {
    let root = session.root.clone();
    let id = session.id.clone();
    let panes = session.panes.max(1);
    let files = session.files.clone();
    let active = session.active;
    if !set_session_cwd(&root) {
        show_session_picker(state);
        return;
    }
    state.borrow_mut().current = Some(session);

    show_grid(state, panes); // builds content, adds + shows the stack page `id`

    // Open the still-valid saved files, tracking where the saved active tab lands
    // once missing / out-of-root files are dropped. `active` indexes the original
    // saved list, so applying it verbatim to the filtered (possibly shorter)
    // notebook would select the wrong tab — or a non-existent page (a silent no-op
    // that leaves the wrong tab active).
    let mut opened = 0u32;
    let mut active_page: Option<u32> = None;
    for (i, f) in files.iter().enumerate() {
        if let Some(f) = restored_session_file(&root, f) {
            open_file(state, &f);
            if Some(i) == active {
                active_page = Some(opened);
            }
            opened += 1;
        }
    }
    if let Some(page) = active_page {
        if let Some(editor) = state.borrow().editor.as_ref() {
            editor.root.set_current_page(Some(page));
        }
    }
    // Remember the live content so its shells keep running while hidden.
    {
        let mut s = state.borrow_mut();
        if let (
            Some(grid),
            Some(sidebar),
            Some(editor),
            Some(shots_panel),
            Some(shots_capture),
            Some(center),
            Some(content),
        ) = (
            s.grid.clone(),
            s.sidebar.clone(),
            s.editor.clone(),
            s.shots_panel.clone(),
            s.shots_capture.clone(),
            s.center_paned.clone(),
            s.content_paned.clone(),
        ) {
            s.live.insert(
                id.clone(),
                LiveContent {
                    grid,
                    sidebar,
                    editor,
                    shots_panel,
                    shots_capture,
                    center,
                    content,
                },
            );
        }
    }
    clear_picker_page(state);
    refresh_session_menu(state);
}

/// Reveal an already-built session: flip the stack to its page (its shells keep
/// running), restore the active handles, and move the shared clipboard into it.
fn reveal_session(state: &Shared, session: Session) {
    let id = session.id.clone();
    if !set_session_cwd(&session.root) {
        show_session_picker(state);
        return;
    }
    // Swap the active handles to this session's live content under a short borrow.
    let (stack, shots_panel, shots_active) = {
        let mut s = state.borrow_mut();
        if let Some(lc) = s.live.get(&id).cloned() {
            s.grid = Some(lc.grid);
            s.sidebar = Some(lc.sidebar);
            s.editor = Some(lc.editor);
            s.shots_panel = Some(lc.shots_panel);
            s.shots_capture = Some(lc.shots_capture);
            s.center_paned = Some(lc.center);
            s.content_paned = Some(lc.content);
        }
        s.current = Some(session);
        (
            s.stack.clone(),
            s.shots_panel.clone(),
            s.shots_btn.is_active(),
        )
    };
    // Flip the stack with no borrow held: mapping the revealed page can re-enter
    // GTK signal handlers, and holding a borrow across that risks a RefCell panic.
    stack.set_visible_child_name(&id);
    if let Some(p) = shots_panel.as_ref() {
        p.set_visible(shots_active);
    }
    reparent_clipboard(state);
    // Re-apply the current font/scale straight to this grid's terminals. A reveal
    // changes neither theme, font nor scale, so the stylesheet is identical to the
    // one already installed — skip apply_view's full CSS rebuild + reparse, which
    // re-styles every widget in the window and is a visible hitch on every switch.
    let (font, size, scale) = {
        let s = state.borrow();
        (s.cfg.font.clone(), s.cfg.font_size, s.cfg.scale)
    };
    if let Some(g) = state.borrow().grid.as_ref() {
        g.apply_font(&font, size, scale);
        g.grab_focused();
    }
    clear_picker_page(state);
    refresh_session_menu(state);
}

fn set_session_cwd(root: &Path) -> bool {
    if let Err(e) = std::env::set_current_dir(root) {
        eprintln!("tessera: could not enter session root {}: {e}", root.display());
        return false;
    }
    true
}

fn restored_session_file(root: &Path, file: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let file = file.canonicalize().ok()?;
    (file.is_file() && file.starts_with(root)).then_some(file)
}

/// Move the shared clipboard strip into the active session's sidebar (after the
/// file tree, before the screenshots strip).
fn reparent_clipboard(state: &Shared) {
    let s = state.borrow();
    let Some(clip) = s.clipboard.clone() else {
        return;
    };
    let Some(sidebar) = s.sidebar.as_ref() else {
        return;
    };
    if clip.root.parent().is_some() {
        clip.root.unparent();
    }
    let first = sidebar.root.first_child();
    sidebar.root.insert_child_after(&clip.root, first.as_ref());
}

/// Drop a session's live content (its shells die) and remove its stack page.
fn drop_live(state: &Shared, id: &str) {
    let (stack, child) = {
        let mut s = state.borrow_mut();
        s.live.remove(id);
        let child = s.stack.child_by_name(id);
        (s.stack.clone(), child)
    };
    // Remove with no borrow held: destroying the page can re-enter GTK handlers.
    if let Some(child) = child {
        stack.remove(&child);
    }
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
        // Pane count changed → tear the live page down and rebuild it (reveal
        // alone would keep the old terminal count).
        drop_live(state, &s.id);
        build_session(state, s);
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
        .unwrap_or_else(|| "Tessera".to_string());
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
    // Clone config so we don't hold a borrow while the grid is built.
    let cfg = {
        let s = state.borrow();
        s.cfg.clone()
    };
    let session_id = state
        .borrow()
        .current
        .as_ref()
        .map(|s| s.id.clone())
        .unwrap_or_default();
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
    let on_empty: crate::grid::EmptyFn = {
        let weak = Rc::downgrade(state);
        let session_id = session_id.clone();
        Rc::new(move || {
            let Some(st) = weak.upgrade() else {
                return;
            };
            let (active, window) = {
                let s = st.borrow();
                (
                    s.current.as_ref().is_some_and(|s| s.id == session_id),
                    s.window.clone(),
                )
            };
            if active {
                window.close();
            } else {
                drop_live(&st, &session_id);
            }
        })
    };
    let grid = Grid::new(n, &cfg, on_open, on_empty);
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

    // Dragging the editor or sidebar divider resizes the terminals too, but those
    // dividers aren't part of the grid's paned tree — wire them to the same reflow
    // suppression so a live resize (including while a pane is zoomed) doesn't
    // garble the focused terminal.
    {
        let weak = Rc::downgrade(state);
        center.connect_position_notify(move |_| {
            if let Some(st) = weak.upgrade() {
                if let Some(g) = st.borrow().grid.as_ref() {
                    g.on_external_resize();
                }
            }
        });
    }
    {
        let weak = Rc::downgrade(state);
        content.connect_position_notify(move |_| {
            if let Some(st) = weak.upgrade() {
                if let Some(g) = st.borrow().grid.as_ref() {
                    g.on_external_resize();
                }
            }
        });
    }

    // Wrap the content with Frame's annotation layer (shown over the content
    // only while editing a capture), and embed the screenshots gallery at the
    // bottom of the file sidebar.
    let window = state.borrow().window.clone();
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
    // Add this session's content as a stack page and show it. Switching to a
    // different session later just flips the visible page, so these shells keep
    // running in the background. Done with no borrow held: mapping the page can
    // re-enter GTK signal handlers.
    let (stack, id, old) = {
        let s = state.borrow();
        let id = s.current.as_ref().map(|c| c.id.clone()).unwrap_or_default();
        let old = s.stack.child_by_name(&id);
        (s.stack.clone(), id, old)
    };
    if let Some(old) = old {
        stack.remove(&old);
    }
    stack.add_named(&bridge.root, Some(&id));
    stack.set_visible_child_name(&id);

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

    // Optionally open a file at startup (TESSERA_OPEN=path) — preview/testing aid.
    if let Some(path) = std::env::var_os("TESSERA_OPEN") {
        open_file(state, std::path::Path::new(&path));
    }

    // Apply the current font size + UI scale to the freshly built terminals.
    apply_view(state);

    // Restore the saved split sizes once the window is mapped and settled.
    restore_session_layout(state);
}

/// Lay out a freshly-built session, then spawn its shells — in that order, so each
/// shell starts at its pane's *final* size.
///
/// The terminals are built empty; their shells aren't spawned until the layout has
/// fully settled. That's the root fix for open-time prompt garble: if a shell
/// spawns before the window is maximized and the splits are restored, it prints
/// its prompt at the wrong size and every subsequent resize reflows it (and the
/// shell reprints its prompt on SIGWINCH), leaving stacked prompts and dead space.
/// Spawning last means the prompt is printed exactly once, at the right size.
///
/// Splits must also be applied outer-first (sidebar/editor before the terminal
/// grid), or the grid is sized against a transient width. Allocations only update
/// on the next layout pass, so this runs as a short poll that waits for each step
/// to settle before doing the next.
fn restore_session_layout(state: &Shared) {
    use gtk4::glib::ControlFlow;
    // Capture this session's handles + restore data up front, so switching to
    // another session mid-restore can't redirect the poll onto its widgets. The
    // captured widgets stay valid (just hidden) while another page is shown.
    let (grid, center, content, divisors, editor_split, sidebar_width, id) = {
        let s = state.borrow();
        let Some(grid) = s.grid.clone() else {
            return;
        };
        (
            grid,
            s.center_paned.clone(),
            s.content_paned.clone(),
            s.current
                .as_ref()
                .map(|c| c.divisors.clone())
                .unwrap_or_default(),
            s.current.as_ref().and_then(|c| c.editor_split),
            s.current.as_ref().and_then(|c| c.sidebar_width),
            s.current.as_ref().map(|c| c.id.clone()).unwrap_or_default(),
        )
    };
    let weak = Rc::downgrade(state);
    // Spawn the shells once layout has settled — but only if this grid is still the
    // session's installed grid, and focus it only if the session is on screen.
    //
    // If the page was torn down and rebuilt while we were polling (e.g. an Alt+digit
    // pane-count change, which `drop_live`s then rebuilds the same id), this captured
    // grid is now detached. Spawning into it would start orphan shells — re-running
    // the startup command in panes no one will ever see — and `grab_focused` could
    // even steal focus from the live grid. The freshly-built grid has its own poll,
    // so we simply bail here.
    let finish = move |grid: &Grid| {
        let Some(st) = weak.upgrade() else {
            return;
        };
        let (still_installed, on_screen) = {
            let s = st.borrow();
            (
                s.live.get(&id).is_some_and(|lc| lc.grid.same(grid)),
                s.current.as_ref().is_some_and(|c| c.id == id),
            )
        };
        if !still_installed {
            return;
        }
        grid.spawn_pending();
        if on_screen {
            grid.grab_focused();
        }
    };
    let mut phase = 0u8;
    let mut last_w = -1i32;
    let mut stable = 0u8;
    let mut ticks = 0u32;
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
        ticks += 1;
        // ~5s safety cap: always spawn the shells, even if sizes never settle.
        if ticks > 300 {
            finish(&grid);
            return ControlFlow::Break;
        }
        let center_w = center.as_ref().map(|c| c.width()).unwrap_or(0);
        let cw = grid.container_size().0;
        match phase {
            // Phase 0: wait for the window to settle (post-maximize), then apply
            // the outer splits against their final width. 3 stable ticks (~48ms)
            // rides out the maximize without over-waiting; even if it fired early,
            // the shells aren't spawned until phase 2 so there's no garble — only,
            // at worst, slightly-off split ratios.
            0 => {
                stable = if center_w > 1 && center_w == last_w {
                    stable + 1
                } else {
                    0
                };
                last_w = center_w;
                if stable < 3 {
                    return ControlFlow::Continue;
                }
                if let (Some(content), Some(sw)) = (content.as_ref(), sidebar_width) {
                    content.set_position(sw);
                }
                if let (Some(center), Some(ratio)) = (center.as_ref(), editor_split) {
                    if center.end_child().is_some() {
                        center.set_position((ratio * center_w as f64).round() as i32);
                    }
                }
                phase = 1;
                last_w = -1;
                stable = 0;
                ControlFlow::Continue
            }
            // Phase 1: wait for the grid width to settle after the outer splits,
            // then size the terminal splits against their final width.
            1 => {
                stable = if cw > 1 && cw == last_w {
                    stable + 1
                } else {
                    0
                };
                last_w = cw;
                if stable < 2 {
                    return ControlFlow::Continue;
                }
                if divisors.is_empty() {
                    grid.relayout_positions();
                } else {
                    grid.apply_split_ratios(&divisors);
                }
                phase = 2;
                last_w = -1;
                stable = 0;
                ControlFlow::Continue
            }
            // Phase 2: wait for the terminal splits to settle, then spawn the
            // shells — every pane is now at its final size.
            _ => {
                stable = if cw > 1 && cw == last_w {
                    stable + 1
                } else {
                    0
                };
                last_w = cw;
                if stable < 2 {
                    return ControlFlow::Continue;
                }
                finish(&grid);
                ControlFlow::Break
            }
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
