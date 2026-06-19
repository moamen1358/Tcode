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
    // Number from the highest shot-N.png already on disk so we never overwrite
    // a screenshot from a previous session (the panel persists across restarts).
    let n = next_shot_number(&dir);
    let path = dir.join(format!("shot-{n}.png"));
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    surface.write_to_png(&mut file).map_err(|e| e.to_string())?;

    let out = Pixbuf::from_file(&path).map_err(|e| e.to_string())?;
    Ok((path, out))
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
    max + 1
}
