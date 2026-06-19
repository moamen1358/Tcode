//! Background rendering of PDFs and office documents to page-image files. Office
//! docs are first converted to PDF with `soffice --headless`, then every PDF is
//! rasterised with `pdftoppm`. Rendered pages are cached under
//! `~/.cache/tessera/preview/<key>/` (key = path + mtime + size), so re-opening
//! is instant. Rendering runs on a worker thread; pages are delivered to the GTK
//! main thread over an `async-channel` (see `editor::build_pages`).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
    /// Rendering failed (human-readable reason).
    Error(String),
}

/// Render `path` on a background thread; results arrive on `tx`. Setting
/// `cancel` (e.g. when the tab is closed) stops delivery early.
pub fn start_render(path: PathBuf, tx: async_channel::Sender<Msg>, cancel: Arc<AtomicBool>) {
    std::thread::spawn(move || match render(&path, &tx, &cancel) {
        Ok(()) => {
            let _ = tx.send_blocking(Msg::Done);
        }
        Err(e) => {
            let _ = tx.send_blocking(Msg::Error(e));
        }
    });
}

fn render(path: &Path, tx: &async_channel::Sender<Msg>, cancel: &AtomicBool) -> Result<(), String> {
    let cache = cache_dir(path)?;
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;

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
            office_to_pdf(path, &cache)?
        } else {
            path.to_path_buf()
        };
        if cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        rasterize(&pdf, &cache)?;
        pages = collect_pages(&cache);
        if !pages.is_empty() {
            let _ = std::fs::write(&done, b""); // cache is now complete
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

fn is_office(path: &Path) -> bool {
    path.extension()
        .map(|e| OFFICE_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Convert an office document to PDF in `cache`. A per-file LibreOffice profile
/// (removed afterwards) lets this run even while the user's own LibreOffice is
/// open and lets two conversions run without fighting over a lock.
fn office_to_pdf(path: &Path, cache: &Path) -> Result<PathBuf, String> {
    let profile = cache.join("soffice-profile");
    let status = Command::new("soffice")
        .args(["--headless", "--norestore", "--invisible", "--nologo"])
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .args(["--convert-to", "pdf", "--outdir"])
        .arg(cache)
        .arg(path)
        .status()
        .map_err(|e| format!("soffice not available: {e}"))?;
    if !status.success() {
        if let Err(e) = std::fs::remove_dir_all(&profile) {
            eprintln!("tessera: profile cleanup failed: {e}");
        }
        return Err("document conversion failed".into());
    }
    if let Err(e) = std::fs::remove_dir_all(&profile) {
        eprintln!("tessera: profile cleanup failed: {e}");
    }
    let stem = path.file_stem().map(PathBuf::from).unwrap_or_default();
    let pdf = cache.join(stem.with_extension("pdf"));
    pdf.exists()
        .then_some(pdf)
        .ok_or_else(|| "converted PDF not found".into())
}

fn rasterize(pdf: &Path, cache: &Path) -> Result<(), String> {
    // 200 DPI keeps pages crisp when zoomed in (150 visibly pixelates) while
    // staying light enough to cache and load quickly.
    let status = Command::new("pdftoppm")
        .args(["-png", "-r", "200"])
        .arg(pdf)
        .arg(cache.join("page"))
        .status()
        .map_err(|e| format!("pdftoppm not available: {e}"))?;
    if !status.success() {
        return Err("pdftoppm failed".into());
    }
    Ok(())
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
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".cache")
        });
    Ok(base.join("tessera").join("preview").join(fnv1a(&key)))
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
