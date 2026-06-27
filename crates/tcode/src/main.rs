mod app;
mod clipboard;
mod conductor;
mod csvview;
mod dnd;
mod editor;
mod frame;
mod grid;
mod icons;
mod keys;
mod loader;
mod overlay;
mod pane;
mod preview;
mod session_picker;
mod sidebar;
mod theme;

use gtk4::prelude::*;
use gtk4::{gio, glib, Application};

const APP_ID: &str = "dev.tcode.Tcode";

fn main() -> glib::ExitCode {
    // CLI subcommands, handled before any GTK setup.
    if let Some(arg) = std::env::args().nth(1) {
        match arg.as_str() {
            "update" => return cli_update(),
            "version" | "--version" | "-V" => {
                println!("tcode {}", env!("CARGO_PKG_VERSION"));
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

    // Optional pane count from argv[1] (e.g. `tcode 4`). Read it ourselves so
    // GTK never sees it — we hand GTK a clean argv to avoid file-open parsing.
    let preset: Option<usize> = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .filter(|n| (1..=16).contains(n));

    // NON_UNIQUE: each launch is its own independent window (no single-instance
    // handoff to an already-running Tcode).
    let app = Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| app::build(app, preset));
    app.run_with_args(&["tcode"])
}

/// `tcode update`: fetch the latest GitHub release and install its `.deb` — no
/// source checkout needed. Downloads with `curl`, installs with `pkexec apt-get`
/// (you're prompted for your password once).
fn cli_update() -> glib::ExitCode {
    const REPO: &str = "moamen1358/Tcode";
    let current = env!("CARGO_PKG_VERSION");
    println!("Tcode {current} — checking for a newer release…");

    let api = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let body = match std::process::Command::new("curl")
        .args(["-fsSL", "-A", "tcode-update", "-H", "Accept: application/vnd.github+json"])
        .arg(&api)
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).into_owned(),
        _ => {
            eprintln!("tcode: couldn't reach GitHub (need `curl` and a connection).");
            return glib::ExitCode::FAILURE;
        }
    };

    let tag = json_string(&body, "tag_name").unwrap_or_default();
    let latest = tag.trim_start_matches('v');
    if latest.is_empty() {
        eprintln!("tcode: no published release found yet.");
        return glib::ExitCode::FAILURE;
    }
    if latest == current {
        println!("Already up to date ({current}).");
        return glib::ExitCode::SUCCESS;
    }
    let Some(url) = deb_asset_url(&body) else {
        eprintln!("tcode: release {tag} has no .deb to install.");
        return glib::ExitCode::FAILURE;
    };

    println!("Updating {current} → {latest}…");
    // Download into a private 0700 dir we own — not a predictable name in the
    // world-writable temp dir — so no other local user can swap in a malicious .deb
    // before it is handed to the root `pkexec apt-get install`.
    let tmpdir = match private_temp_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("tcode: could not create a private temp dir: {e}");
            return glib::ExitCode::FAILURE;
        }
    };
    let deb = tmpdir.join("tcode-latest.deb");
    let downloaded = std::process::Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&deb)
        .arg(&url)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !downloaded {
        eprintln!("tcode: download failed.");
        let _ = std::fs::remove_dir_all(&tmpdir);
        return glib::ExitCode::FAILURE;
    }

    println!("Installing (you'll be asked for your password once)…");
    let installed = std::process::Command::new("pkexec")
        .args(["apt-get", "install", "-y"])
        .arg(&deb)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let _ = std::fs::remove_dir_all(&tmpdir);

    if installed {
        println!("Updated to {latest}. Restart Tcode to use the new version.");
        glib::ExitCode::SUCCESS
    } else {
        eprintln!("tcode: install failed. Download the .deb manually:\n  {url}");
        glib::ExitCode::FAILURE
    }
}

/// Create a freshly-made private (0700) temp directory we own, mkdtemp-style, and
/// return its path. A unique 0700 dir — rather than a fixed name in the
/// world-writable temp dir — stops another local user from planting or swapping the
/// `.deb` that is then fed to the root `pkexec apt-get install`. `mkdir` is atomic
/// and fails on a pre-existing name (incl. a planted symlink), so we retry on the
/// vanishingly unlikely collision.
fn private_temp_dir() -> std::io::Result<std::path::PathBuf> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let base = std::env::temp_dir();
    let pid = std::process::id();
    for attempt in 0..32u64 {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = base.join(format!("tcode-update.{pid:x}.{nanos:x}.{attempt:x}"));
        let mut b = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            b.mode(0o700);
        }
        match b.create(&dir) {
            Ok(()) => return Ok(dir),
            // Name already taken (collision or attacker-planted) — try another.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "could not create a unique private temp dir",
    ))
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

/// Usage text for `tcode --help`.
fn print_usage() {
    print!(
"Tcode — a borderless tiling-terminal workspace.

Usage:
  tcode [N]        open with N panes (1-16); omit N for the session picker
  tcode update     update to the latest version and reinstall
  tcode --version  print the version
  tcode --help     show this help

Keybindings & config: https://github.com/moamen1358/Tcode
"
    );
}

/// One-time migration of data written under the old "loom" name into the new
/// "tcode" dirs, so existing sessions, clipboard history, and screenshots survive
/// the rename. Each move only happens if the new location doesn't exist yet.
fn migrate_legacy_data() {
    for base in [
        glib::user_config_dir(),
        glib::user_cache_dir(),
        glib::user_data_dir(),
    ] {
        let (old, new) = (base.join("loom"), base.join("tcode"));
        if old.is_dir() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
    // The screenshots subdir was renamed too (bridgeshot -> frame).
    let cache = glib::user_cache_dir().join("tcode");
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
    let dir = glib::user_data_dir().join("fonts").join("tcode");
    let file = dir.join("MartianMono.ttf");
    if file.exists() {
        return;
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        // Atomic write so a partial/interrupted copy never leaves a corrupt font, and
        // concurrent NON_UNIQUE launches converge on a complete file.
        let _ = tcode_core::fsutil::atomic_write(
            &file,
            include_bytes!("../assets/fonts/MartianMono.ttf"),
            0o644,
        );
    }
}
