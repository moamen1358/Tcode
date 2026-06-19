mod app;
mod bridgeshot;
mod clipboard;
mod csvview;
mod dnd;
mod editor;
mod grid;
mod icons;
mod keys;
mod loader;
mod pane;
mod picker;
mod preview;
mod sidebar;
mod theme;

use gtk4::prelude::*;
use gtk4::{gio, glib, Application};

const APP_ID: &str = "dev.tessera.Tessera";

fn main() -> glib::ExitCode {
    // Make the bundled default font available before GTK initializes fontconfig.
    ensure_bundled_font();

    // Optional pane count from argv[1] (e.g. `tessera 4`). Read it ourselves so
    // GTK never sees it — we hand GTK a clean argv to avoid file-open parsing.
    let preset: Option<usize> = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|n| (1..=16).contains(n));

    // NON_UNIQUE: each launch is its own independent window (no single-instance
    // handoff to an already-running Tessera).
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| app::build(app, preset));
    app.run_with_args(&["tessera"])
}

/// Write the bundled `Martian Mono` into the user font directory if it isn't
/// already there — done before GTK initializes fontconfig, so its startup scan
/// picks the file up and the app's default font renders on a fresh machine
/// without a manual install.
fn ensure_bundled_font() {
    let dir = glib::user_data_dir().join("fonts").join("tessera");
    let file = dir.join("MartianMono.ttf");
    if file.exists() {
        return;
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(&file, include_bytes!("../assets/fonts/MartianMono.ttf"));
    }
}
