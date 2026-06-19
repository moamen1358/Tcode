//! Application state and window wiring: borderless window, picker ↔ (sidebar + grid),
//! and the image drag-and-drop target.

use gtk4::gdk::{DragAction, FileList};
use gtk4::prelude::*;
use gtk4::{
    gio, glib, Application, ApplicationWindow, Button, DropTarget, HeaderBar, Orientation, Paned,
    ToggleButton,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use tessera_core::config::Config;

use crate::editor::Editor;
use crate::grid::Grid;
use crate::sidebar::{shell_quote, Sidebar};
use crate::{keys, picker, theme};

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
    /// BridgeShot capture action (region-select → annotate), set once the grid
    /// is built; the titlebar camera button invokes it.
    pub shots_capture: Option<Rc<dyn Fn()>>,
}

pub type Shared = Rc<RefCell<State>>;

pub fn build(app: &Application, preset: Option<usize>) {
    let cfg = Config::load();
    theme::install_css(&cfg.theme, &cfg.font, cfg.font_size);

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

    // BridgeShot capture: region-select a screenshot, annotate, save to the panel.
    let capture_btn = Button::from_icon_name("camera-photo-symbolic");
    capture_btn.set_tooltip_text(Some("Capture a screenshot"));
    capture_btn.add_css_class("flat");
    header.pack_end(&capture_btn);

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
    }));

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

    match preset {
        Some(n) => show_grid(&state, n),
        None => show_picker(&state),
    }
    window.present();
}

pub fn show_picker(state: &Shared) {
    let st = state.clone();
    let widget = picker::build(move |n| show_grid(&st, n));
    state.borrow().window.set_child(Some(&widget));
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

    // Wrap the content with BridgeShot's annotation layer (shown over the content
    // only while editing a capture), and embed the screenshots gallery at the
    // bottom of the file sidebar.
    let bridge = crate::bridgeshot::integrate(&window, &content);
    sidebar.root.append(&bridge.panel_root);
    window.set_child(Some(&bridge.root));

    {
        let mut s = state.borrow_mut();
        s.grid = Some(grid);
        s.sidebar = Some(sidebar);
        s.editor = Some(editor);
        s.shots_panel = Some(bridge.panel_root.clone());
        s.shots_capture = Some(bridge.capture.clone());
    }
    // Respect the current toggle state for the freshly built panel.
    bridge
        .panel_root
        .set_visible(state.borrow().shots_btn.is_active());

    // Optionally open a file at startup (TESSERA_OPEN=path) — preview/testing aid.
    if let Some(path) = std::env::var_os("TESSERA_OPEN") {
        open_file(state, std::path::Path::new(&path));
    }

    // Grab keyboard focus once the window is mapped (COSMIC drops a focus
    // grabbed before present()).
    let st = state.clone();
    gtk4::glib::idle_add_local_once(move || {
        if let Some(g) = st.borrow().grid.as_ref() {
            g.relayout_positions();
            g.grab_focused();
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
    let drop = DropTarget::new(glib::Type::INVALID, DragAction::COPY);
    drop.set_types(&[FileList::static_type(), gio::File::static_type()]);
    let st = state.clone();
    drop.connect_drop(move |_t, value, _x, _y| {
        let mut paths: Vec<PathBuf> = Vec::new();
        if let Ok(list) = value.get::<FileList>() {
            for f in list.files() {
                if let Some(p) = f.path() {
                    paths.push(p);
                }
            }
        } else if let Ok(f) = value.get::<gio::File>() {
            if let Some(p) = f.path() {
                paths.push(p);
            }
        } else {
            return false;
        }
        if paths.is_empty() {
            return false;
        }
        let joined = paths
            .iter()
            .map(|p| shell_quote(p))
            .collect::<Vec<_>>()
            .join(" ");
        if let Some(g) = st.borrow().grid.as_ref() {
            g.feed_focused(&joined);
        }
        true
    });
    widget.add_controller(drop);
}
