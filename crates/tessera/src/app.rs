//! Application state and window wiring: borderless window, picker ↔ (sidebar + grid),
//! and the image drag-and-drop target.

use gtk4::gdk::{DragAction, FileList};
use gtk4::prelude::*;
use gtk4::{
    gio, glib, Application, ApplicationWindow, Box as GtkBox, Button, DropTarget, HeaderBar,
    Orientation, ToggleButton,
};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use tessera_core::config::Config;

use crate::grid::Grid;
use crate::sidebar::{shell_quote, Sidebar};
use crate::{keys, picker, theme};

pub struct State {
    pub window: ApplicationWindow,
    pub cfg: Config,
    pub grid: Option<Grid>,
    pub sidebar: Option<Sidebar>,
    pub sidebar_btn: ToggleButton,
}

pub type Shared = Rc<RefCell<State>>;

pub fn build(app: &Application, preset: Option<usize>) {
    let cfg = Config::load();
    theme::install_css(&cfg.theme.accent, &cfg.theme.background);

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

    window.set_titlebar(Some(&header));

    let state: Shared = Rc::new(RefCell::new(State {
        window: window.clone(),
        cfg,
        grid: None,
        sidebar: None,
        sidebar_btn: sidebar_btn.clone(),
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
    let grid = Grid::new(n, &cfg, &window);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let sidebar = Sidebar::new(&cwd, state);
    // Respect the current toggle state for the freshly built sidebar.
    sidebar.root.set_visible(state.borrow().sidebar_btn.is_active());

    install_image_drop(&grid.root, state);
    grid.root.set_hexpand(true);
    grid.root.set_vexpand(true);

    let content = GtkBox::new(Orientation::Horizontal, 0);
    content.append(&sidebar.root);
    content.append(&grid.root);
    window.set_child(Some(&content));

    {
        let mut s = state.borrow_mut();
        s.grid = Some(grid);
        s.sidebar = Some(sidebar);
    }

    // Grab keyboard focus once the window is mapped (COSMIC drops a focus
    // grabbed before present()).
    let st = state.clone();
    gtk4::glib::idle_add_local_once(move || {
        if let Some(g) = st.borrow().grid.as_ref() {
            g.grab_focused();
        }
    });
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
