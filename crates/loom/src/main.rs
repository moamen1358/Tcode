mod app;
mod clipboard;
mod csvview;
mod dnd;
mod editor;
mod frame;
mod grid;
mod icons;
mod keys;
mod loader;
mod pane;
mod preview;
mod session_picker;
mod sidebar;
mod theme;

use gtk4::prelude::*;
use gtk4::{gio, glib, Application};

const APP_ID: &str = "dev.loom.Loom";

fn main() -> glib::ExitCode {
    // Carry over data written under the old name so the rename loses nothing.
    migrate_legacy_data();
    // Make the bundled default font available before GTK initializes fontconfig.
    ensure_bundled_font();

    // Optional pane count from argv[1] (e.g. `loom 4`). Read it ourselves so
    // GTK never sees it — we hand GTK a clean argv to avoid file-open parsing.
    let preset: Option<usize> = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|n| (1..=16).contains(n));

    // NON_UNIQUE: each launch is its own independent window (no single-instance
    // handoff to an already-running Loom).
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| app::build(app, preset));
    app.run_with_args(&["loom"])
}

/// One-time migration of data written under the old "tessera" name into the new
/// "loom" dirs, so existing sessions, clipboard history, and screenshots survive
/// the rename. Each move only happens if the new location doesn't exist yet.
fn migrate_legacy_data() {
    for base in [
        glib::user_config_dir(),
        glib::user_cache_dir(),
        glib::user_data_dir(),
    ] {
        let (old, new) = (base.join("tessera"), base.join("loom"));
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
    // The screenshots subdir was renamed too (bridgeshot -> frame).
    let cache = glib::user_cache_dir().join("loom");
    let (old, new) = (cache.join("bridgeshot"), cache.join("frame"));
    if old.is_dir() && !new.exists() {
        let _ = std::fs::rename(&old, &new);
    }
}

/// Write the bundled `Martian Mono` into the user font directory if it isn't
/// already there — done before GTK initializes fontconfig, so its startup scan
/// picks the file up and the app's default font renders on a fresh machine
/// without a manual install.
fn ensure_bundled_font() {
    let dir = glib::user_data_dir().join("fonts").join("loom");
    let file = dir.join("MartianMono.ttf");
    if file.exists() {
        return;
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(&file, include_bytes!("../assets/fonts/MartianMono.ttf"));
    }
}
