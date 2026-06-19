//! Background rendering of PDFs and office documents to page-image files. Office
//! docs are first converted to PDF with `soffice --headless`, then every PDF is
//! rasterised with `pdftoppm`. Rendered pages are cached under
//! `~/.cache/loom/preview/<key>/` (key = path + mtime + size), so re-opening
//! is instant. Rendering runs on a worker thread; pages are delivered to the GTK
//! main thread over an `async-channel` (see `editor::build_pages`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk4::gio::prelude::FileExt; // gio::File::uri

const OFFICE_EXTS: &[&str] = &[
    "doc", "docx", "odt", "rtf", "ppt", "pptx", "odp", "xls", "xlsx", "ods",
];

/// Worker → UI messages.
pub enum Msg {
    /// Total page count is known.
    Pages(usize),
    /// One rendered page image, delivered in order.
    Page(PathBuf),
    /// Rendering finished successfully.
    Done,
    /// Rendering was cancelled before completion.
    Cancelled,
    /// Rendering failed (human-readable reason).
    Error(String),
}

/// Render `path` on a background thread; results arrive on `tx`. Setting
/// `cancel` (e.g. when the tab is closed) stops delivery early.
pub fn start_render(path: PathBuf, tx: async_channel::Sender<Msg>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || match render(&path, &tx, &cancel) {
        Ok(()) if cancel.load(Ordering::Acquire) => {
            let _ = tx.send_blocking(Msg::Cancelled);
        }
        Ok(()) => {
            let _ = tx.send_blocking(Msg::Done);
        }
        Err(e) => {
            let _ = tx.send_blocking(Msg::Error(e));
        }
    });
}

fn render(path: &Path, tx: &async_channel::Sender<Msg>, cancel: &AtomicBool) -> Result<(), String> {
    // Canonicalize before anything touches a subprocess: an absolute path can't
    // be mistaken for a CLI flag by soffice/pdftoppm (a file named e.g. "-x.pdf"
    // would otherwise be parsed as an option, i.e. argument injection).
    let canon = path.canonicalize().map_err(|e| e.to_string())?;
    let path = canon.as_path();
    let cache = cache_dir(path)?;
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    make_private_dir(&cache);

    // Only trust the cache if a previous render finished completely. Without this
    // marker, a render killed part-way (OOM, tab closed, crash) would leave a few
    // page-*.png behind that look like a full cache, so the document would show
    // permanently truncated and never re-render.
    let done = cache.join(".done");
    let mut pages = if done.exists() {
        collect_pages(&cache)
    } else {
        Vec::new()
    };
    if pages.is_empty() {
        let pdf = if is_office(path) {
            match office_to_pdf(path, &cache, cancel)? {
                Some(p) => p,
                None => return Ok(()), // tab closed mid-conversion
            }
        } else {
            path.to_path_buf()
        };
        if !rasterize(&pdf, &cache, cancel)? {
            return Ok(()); // cancelled
        }
        pages = collect_pages(&cache);
        if !pages.is_empty() {
            let _ = std::fs::write(&done, b""); // cache is now complete
            prune_cache(&cache); // keep the regenerable preview cache bounded
        }
    }
    if pages.is_empty() {
        return Err("no pages were produced".into());
    }

    if tx.send_blocking(Msg::Pages(pages.len())).is_err() {
        return Ok(());
    }
    for p in pages {
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        if tx.send_blocking(Msg::Page(p)).is_err() {
            return Ok(());
        }
    }
    Ok(())
}

pub fn is_office(path: &Path) -> bool {
    path.extension()
        .map(|e| OFFICE_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Run `cmd` to completion, killing it if `cancel` flips (so closing a tab
/// mid-render doesn't leave soffice/pdftoppm running). Returns `None` if it was
/// cancelled before finishing.
fn run_cancellable(
    mut cmd: Command,
    cancel: &AtomicBool,
) -> Result<Option<std::process::ExitStatus>, String> {
    // Cap wall-clock time so a hung or pathological document can't pin a render
    // worker (and a soffice/pdftoppm process) indefinitely.
    const MAX_RENDER: std::time::Duration = std::time::Duration::from_secs(120);
    let start = std::time::Instant::now();
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    loop {
        if cancel.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        if start.elapsed() > MAX_RENDER {
            let _ = child.kill();
            let _ = child.wait();
            return Err("render timed out".into());
        }
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => return Ok(Some(status)),
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
}

/// Convert an office document to PDF in `cache`. A per-file LibreOffice profile
/// (removed afterwards) lets this run even while the user's own LibreOffice is
/// open and lets two conversions run without fighting over a lock. Returns `None`
/// if cancelled mid-conversion.
fn office_to_pdf(
    path: &Path,
    cache: &Path,
    cancel: &AtomicBool,
) -> Result<Option<PathBuf>, String> {
    let profile = cache.join("soffice-profile");
    // A properly percent-encoded file:// URI (not a hand-built "file://" + path):
    // the cache dir lives under the XDG cache home / username, which can contain
    // spaces, '#' or '%'. Those make a raw "file://{path}" an invalid URL, and
    // LibreOffice then silently ignores the per-file profile.
    let profile_uri = gtk4::gio::File::for_path(&profile).uri();
    let mut cmd = Command::new("soffice");
    cmd.args(["--headless", "--norestore", "--invisible", "--nologo"])
        .arg(format!("-env:UserInstallation={profile_uri}"))
        .args(["--convert-to", "pdf", "--outdir"])
        .arg(cache)
        .arg(path);
    let status = run_cancellable(cmd, cancel).map_err(|e| format!("soffice failed: {e}"))?;
    if profile.exists() {
        if let Err(e) = std::fs::remove_dir_all(&profile) {
            eprintln!("loom: profile cleanup failed: {e}");
        }
    }
    let Some(status) = status else {
        return Ok(None); // cancelled
    };
    if !status.success() {
        return Err("document conversion failed".into());
    }
    let stem = path.file_stem().map(PathBuf::from).unwrap_or_default();
    let pdf = cache.join(stem.with_extension("pdf"));
    pdf.exists()
        .then_some(pdf)
        .map(Some)
        .ok_or_else(|| "converted PDF not found".into())
}

/// Rasterize a PDF to `page-N.png` in `cache`. Returns `false` if cancelled.
fn rasterize(pdf: &Path, cache: &Path, cancel: &AtomicBool) -> Result<bool, String> {
    // 200 DPI keeps pages crisp when zoomed in (150 visibly pixelates) while
    // staying light enough to cache and load quickly.
    // Cap rendered pages so a document with tens of thousands of pages can't
    // exhaust CPU/disk (and memory when each PNG is later loaded as a widget).
    let mut cmd = Command::new("pdftoppm");
    cmd.args(["-png", "-r", "200", "-l", "300"])
        .arg(pdf)
        .arg(cache.join("page"));
    let status = run_cancellable(cmd, cancel).map_err(|e| format!("pdftoppm failed: {e}"))?;
    let Some(status) = status else {
        return Ok(false); // cancelled
    };
    if !status.success() {
        return Err("pdftoppm failed".into());
    }
    Ok(true)
}

/// Keep the preview cache bounded: delete all but the most recently modified
/// cache directories under `.../preview` (the page images are regenerable).
fn prune_cache(current: &Path) {
    const KEEP: usize = 20;
    let Some(base) = current.parent() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(base) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            // Real directories only — file_type() doesn't follow symlinks, so a
            // planted symlink here can't send remove_dir_all outside the cache.
            if !e.file_type().ok()?.is_dir() {
                return None;
            }
            let p = e.path();
            let t = e
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            Some((t, p))
        })
        .collect();
    if dirs.len() <= KEEP {
        return;
    }
    dirs.sort_by_key(|(t, _)| *t); // oldest first
    for (_, p) in dirs.iter().take(dirs.len() - KEEP) {
        let _ = std::fs::remove_dir_all(p);
    }
}

fn cache_dir(path: &Path) -> Result<PathBuf, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let key = format!("{}|{}|{}", path.display(), mtime, meta.len());
    Ok(gtk4::glib::user_cache_dir()
        .join("loom")
        .join("preview")
        .join(fnv1a(&key)))
}

fn make_private_dir(dir: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
}

/// FNV-1a 64-bit — a stable cache key, no dependencies.
fn fnv1a(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn collect_pages(cache: &Path) -> Vec<PathBuf> {
    let mut pages: Vec<PathBuf> = std::fs::read_dir(cache)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| {
                    let n = n.to_string_lossy();
                    n.starts_with("page") && n.ends_with(".png")
                })
                .unwrap_or(false)
        })
        .collect();
    // pdftoppm zero-pads page numbers, but sort numerically to be safe.
    pages.sort_by_key(|p| page_number(p));
    pages
}

fn page_number(p: &Path) -> u32 {
    p.file_stem()
        .and_then(|s| s.to_str())
        .map(|s| {
            let d: String = s
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            d.parse::<u32>().unwrap_or(0)
        })
        .unwrap_or(0)
}
