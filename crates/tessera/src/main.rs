mod app;
mod grid;
mod keys;
mod pane;
mod picker;
mod sidebar;
mod theme;

use gtk4::prelude::*;
use gtk4::{glib, Application};

const APP_ID: &str = "dev.tessera.Tessera";

fn main() -> glib::ExitCode {
    // Optional pane count from argv[1] (e.g. `tessera 4`). Read it ourselves so
    // GTK never sees it — we hand GTK a clean argv to avoid file-open parsing.
    let preset: Option<usize> = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|n| (1..=16).contains(n));

    let app = Application::builder().application_id(APP_ID).build();
    app.connect_activate(move |app| app::build(app, preset));
    app.run_with_args(&["tessera"])
}
