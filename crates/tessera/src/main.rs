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

const APP_ID: &str = "dev.tessera.Tessera";

fn main() -> glib::ExitCode {
    // CLI subcommands, handled before any GTK setup.
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "update" => return cli_update(),
            "version" | "--version" | "-V" => {
                println!("tessera {}", env!("CARGO_PKG_VERSION"));
                return glib::ExitCode::SUCCESS;
            }
            "help" | "--help" | "-h" => {
                print_usage();
                return glib::ExitCode::SUCCESS;
            }
            _ => {}
        }
    }

    // Carry over data written under the old name so the rename loses nothing.
    migrate_legacy_data();
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

/// `tessera update`: pull the latest source and reinstall, via the updater the
/// installer recorded under the data dir.
fn cli_update() -> glib::ExitCode {
    let marker = glib::user_data_dir().join("tessera").join("source");
    let dir = match std::fs::read_to_string(&marker) {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            eprint!(
"tessera: don't know where Tessera's source is.
Reinstall from a clone (it records the path for next time):
  git clone https://github.com/moamen1358/tessera
  cd tessera && ./packaging/install.sh
"
            );
            return glib::ExitCode::FAILURE;
        }
    };
    let script = std::path::Path::new(&dir).join("packaging/update.sh");
    if !script.is_file() {
        eprintln!("tessera: updater not found at {}", script.display());
        return glib::ExitCode::FAILURE;
    }
    println!("Updating Tessera from {dir} …");
    match std::process::Command::new("bash").arg(&script).status() {
        Ok(s) if s.success() => glib::ExitCode::SUCCESS,
        Ok(_) => glib::ExitCode::FAILURE,
        Err(e) => {
            eprintln!("tessera: failed to run the updater: {e}");
            glib::ExitCode::FAILURE
        }
    }
}

/// Usage text for `tessera --help`.
fn print_usage() {
    print!(
"Tessera — a borderless tiling-terminal workspace.

Usage:
  tessera [N]        open with N panes (1-16); omit N for the session picker
  tessera update     update to the latest version and reinstall
  tessera --version  print the version
  tessera --help     show this help

Keybindings & config: https://github.com/moamen1358/tessera
"
    );
}

/// One-time migration of data written under the old "loom" name into the new
/// "tessera" dirs, so existing sessions, clipboard history, and screenshots survive
/// the rename. Each move only happens if the new location doesn't exist yet.
fn migrate_legacy_data() {
    for base in [
        glib::user_config_dir(),
        glib::user_cache_dir(),
        glib::user_data_dir(),
    ] {
        let (old, new) = (base.join("loom"), base.join("tessera"));
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
    // The screenshots subdir was renamed too (bridgeshot -> frame).
    let cache = glib::user_cache_dir().join("tessera");
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
    let dir = glib::user_data_dir().join("fonts").join("tessera");
    let file = dir.join("MartianMono.ttf");
    if file.exists() {
        return;
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(&file, include_bytes!("../assets/fonts/MartianMono.ttf"));
    }
}
