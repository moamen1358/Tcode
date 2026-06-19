//! Render the active document (image + annotations) to a PNG and return a
//! Pixbuf of the result for the clipboard.

#![allow(dead_code)] // wired up in the orchestrator task

use std::path::PathBuf;

use gtk4::cairo;
use gtk4::gdk::prelude::GdkCairoContextExt;
use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;

use super::canvas::paint_annotations;
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

    let dir = glib::user_cache_dir().join("tessera").join("bridgeshot");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let n = {
        let mut s = shot.borrow_mut();
        s.exports += 1;
        s.exports
    };
    let path = dir.join(format!("shot-{n}.png"));
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    surface.write_to_png(&mut file).map_err(|e| e.to_string())?;

    let out = Pixbuf::from_file(&path).map_err(|e| e.to_string())?;
    Ok((path, out))
}
