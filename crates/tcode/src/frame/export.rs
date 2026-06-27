//! Render the active document (image + annotations) to a PNG and return a
//! Pixbuf of the result for the clipboard.

use std::path::{Path, PathBuf};

use gtk4::cairo;
use gtk4::gdk::prelude::GdkCairoContextExt;
use gtk4::gdk_pixbuf::Pixbuf;

use super::canvas::paint_annotations;
use super::gallery::shots_dir;
use super::state::Shot;

pub fn export_png(shot: &Shot) -> Result<(PathBuf, Pixbuf), String> {
    let s = shot.borrow();
    let doc = s.active_doc().ok_or("nothing to export")?;
    let pb = doc.pixbuf.clone();
    let (iw, ih) = (pb.width(), pb.height());

    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, iw, ih).map_err(|e| e.to_string())?;
    {
        let cr = cairo::Context::new(&surface).map_err(|e| e.to_string())?;
        cr.set_source_pixbuf(&pb, 0.0, 0.0);
        let _ = cr.paint();
        // scale = 1.0: stroke widths are authored in image space.
        paint_annotations(&cr, &doc.annos, 1.0);
    }
    drop(s);

    let dir = shots_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    // Number from the highest shot-N.png already on disk so we never overwrite a
    // screenshot from a previous session — and, since this is now the shared
    // Pictures/Screenshots folder, no foreign screenshot either (we only ever
    // create new shot-N names). create_new (O_EXCL) so two instances saving at
    // once can't truncate each other's shot; bump the index and retry on a collision.
    let mut n = next_shot_number(&dir);
    let (path, mut file) = loop {
        let path = dir.join(format!("shot-{n}.png"));
        match create_new_file(&path) {
            Ok(f) => break (path, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e.to_string()),
        }
    };
    if let Err(e) = surface.write_to_png(&mut file) {
        let _ = std::fs::remove_file(&path);
        return Err(e.to_string());
    }
    drop(file);

    let out = Pixbuf::from_file(&path).map_err(|e| e.to_string())?;
    Ok((path, out))
}

/// Create the file only if it doesn't already exist (O_EXCL), so two instances
/// saving at once can't truncate each other's shot. Written with the default umask
/// (a normal, readable file) — these live in the user's shared Pictures/Screenshots
/// alongside every other tool's screenshots, so no private 0o600 mode.
fn create_new_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Next free `shot-N` index: one past the highest existing on disk (or 1).
fn next_shot_number(dir: &Path) -> u32 {
    let max = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            e.file_name()
                .to_str()
                .and_then(|n| n.strip_prefix("shot-"))
                .and_then(|n| n.strip_suffix(".png"))
                .and_then(|n| n.parse::<u32>().ok())
        })
        .max()
        .unwrap_or(0);
    // saturating: a planted `shot-4294967295.png` in the shared screenshots dir
    // would otherwise wrap to 0 in release (panic in debug). The O_EXCL create
    // loop still recovers, but don't rely on the overflow.
    max.saturating_add(1)
}
