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

/// `tessera update`: fetch the latest GitHub release and install its `.deb` — no
/// source checkout needed. Downloads with `curl`, installs with `pkexec apt-get`
/// (you're prompted for your password once).
fn cli_update() -> glib::ExitCode {
    const REPO: &str = "moamen1358/Tessera";
    let current = env!("CARGO_PKG_VERSION");
    println!("Tessera {current} — checking for a newer release…");

    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = match std::process::Command::new("curl")
        .args(["-fsSL", "-A", "tessera-update", "-H", "Accept: application/vnd.github+json"])
        .arg(&api)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("tessera: couldn't reach GitHub (need `curl` and a connection).");
            return glib::ExitCode::FAILURE;
        }
    };

    let tag = json_string(&body, "tag_name").unwrap_or_default();
    let latest = tag.trim_start_matches('v');
    if latest.is_empty() {
        eprintln!("tessera: no published release found yet.");
        return glib::ExitCode::FAILURE;
    }
    if latest == current {
        println!("Already up to date ({current}).");
        return glib::ExitCode::SUCCESS;
    }
    let Some(url) = deb_asset_url(&body) else {
        eprintln!("tessera: release {tag} has no .deb to install.");
        return glib::ExitCode::FAILURE;
    };

    println!("Updating {current} → {latest}…");
    let deb = std::env::temp_dir().join("tessera-latest.deb");
    let downloaded = std::process::Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&deb)
        .arg(&url)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !downloaded {
        eprintln!("tessera: download failed.");
        return glib::ExitCode::FAILURE;
    }

    println!("Installing (you'll be asked for your password once)…");
    let installed = std::process::Command::new("pkexec")
        .args(["apt-get", "install", "-y"])
        .arg(&deb)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::fs::remove_file(&deb);

    if installed {
        println!("Updated to {latest}. Restart Tessera to use the new version.");
        glib::ExitCode::SUCCESS
    } else {
        eprintln!("tessera: install failed. Download the .deb manually:\n  {url}");
        glib::ExitCode::FAILURE
    }
}

/// Extract a JSON string field's value (`"field": "value"` -> `value`). Minimal
/// and dependency-free — enough for the small, well-formed GitHub release JSON.
fn json_string(json: &str, field: &str) -> Option<String> {
    let key = format!("\"{field}\"");
    let after = json[json.find(&key)? + key.len()..]
        .trim_start()
        .strip_prefix(':')?
        .trim_start();
    let rest = after.strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

/// The first release-asset `browser_download_url` ending in `.deb`.
fn deb_asset_url(json: &str) -> Option<String> {
    for part in json.split("\"browser_download_url\"").skip(1) {
        let Some(rest) = part.trim_start().strip_prefix(':') else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('"') else {
            continue;
        };
        let Some(end) = rest.find('"') else { continue };
        if rest[..end].ends_with(".deb") {
            return Some(rest[..end].to_string());
        }
    }
    None
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

Keybindings & config: https://github.com/moamen1358/Tessera
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
