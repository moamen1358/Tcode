//! Render PDFs and office documents to page-image files and present them as a
//! scrollable column of pictures. Office docs are converted to PDF with
//! `soffice --headless`, then every PDF is rasterised with `pdftoppm`. Rendered
//! pages are cached under `~/.cache/tessera/preview/<key>/` keyed by the file's
//! path + mtime + size, so re-opening is instant.
//!
//! NOTE: rendering currently runs synchronously when a document tab is opened.
//! That is fine for PDFs (fast) but `soffice` can take a few seconds; an async
//! version is planned.

use std::path::{Path, PathBuf};
use std::process::Command;

use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, ContentFit, Label, Orientation, Picture, PolicyType, ScrolledWindow,
    Widget,
};

const OFFICE_EXTS: &[&str] = &[
    "doc", "docx", "odt", "rtf", "ppt", "pptx", "odp", "xls", "xlsx", "ods", "csv",
];

/// A scrollable column of rendered pages for a PDF or office document.
pub fn document_viewer(path: &Path) -> Widget {
    let column = GtkBox::new(Orientation::Vertical, 14);
    column.set_halign(Align::Center);
    column.set_margin_top(12);
    column.set_margin_bottom(12);
    column.add_css_class("doc-view");

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&column)
        .build();

    match render_pages(path) {
        Ok(pages) if !pages.is_empty() => {
            for page in pages {
                let pic = Picture::for_filename(&page);
                pic.set_can_shrink(true);
                pic.set_content_fit(ContentFit::Contain);
                pic.set_size_request(820, -1);
                pic.add_css_class("doc-page");
                column.append(&pic);
            }
        }
        Ok(_) => column.append(&centered("No pages to display")),
        Err(e) => column.append(&centered(&format!("Could not render preview:\n{e}"))),
    }
    scroller.upcast()
}

fn centered(text: &str) -> Label {
    let l = Label::new(Some(text));
    l.set_halign(Align::Center);
    l.set_valign(Align::Center);
    l.set_vexpand(true);
    l.add_css_class("fallback-meta");
    l
}

fn render_pages(path: &Path) -> Result<Vec<PathBuf>, String> {
    let cache = cache_dir(path)?;
    std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
    let cached = collect_pages(&cache);
    if !cached.is_empty() {
        return Ok(cached);
    }
    let pdf = if is_office(path) {
        office_to_pdf(path, &cache)?
    } else {
        path.to_path_buf()
    };
    pdftoppm(&pdf, &cache)?;
    Ok(collect_pages(&cache))
}

fn is_office(path: &Path) -> bool {
    path.extension()
        .map(|e| OFFICE_EXTS.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
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

fn office_to_pdf(path: &Path, cache: &Path) -> Result<PathBuf, String> {
    let profile = cache.join("soffice-profile");
    let status = Command::new("soffice")
        .arg("--headless")
        .arg(format!("-env:UserInstallation=file://{}", profile.display()))
        .arg("--convert-to")
        .arg("pdf")
        .arg("--outdir")
        .arg(cache)
        .arg(path)
        .status()
        .map_err(|e| format!("soffice not available: {e}"))?;
    if !status.success() {
        return Err("document conversion failed".into());
    }
    let stem = path.file_stem().map(PathBuf::from).unwrap_or_default();
    let pdf = cache.join(stem.with_extension("pdf"));
    pdf.exists()
        .then_some(pdf)
        .ok_or_else(|| "converted PDF not found".into())
}

fn pdftoppm(pdf: &Path, cache: &Path) -> Result<(), String> {
    let status = Command::new("pdftoppm")
        .arg("-png")
        .arg("-r")
        .arg("150")
        .arg(pdf)
        .arg(cache.join("page"))
        .status()
        .map_err(|e| format!("pdftoppm not available: {e}"))?;
    if !status.success() {
        return Err("pdftoppm failed".into());
    }
    Ok(())
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
    pages.sort_by_key(|p| page_number(p));
    pages
}

fn page_number(p: &Path) -> u32 {
    p.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.rsplit('-').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}
