//! Image capture for Frame: the XDG screenshot portal (any window/region,
//! Wayland-safe) with the Tcode self-snapshot as a fallback.

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::ApplicationWindow;

/// What a capture attempt produced.
enum Capture {
    /// The user picked a window/region — annotate this image.
    Image(Pixbuf),
    /// The user dismissed the picker (or it errored after opening) — do nothing,
    /// so cancelling a capture never drops you into the annotation canvas.
    Abort,
    /// The screenshot portal service is unreachable — fall back to snapshotting
    /// Tcode's own window so capture still works on a portal-less system.
    NoPortal,
}

/// Capture ANY window/region via the desktop portal. COSMIC shows its own
/// interactive picker; the chosen area comes back as a PNG we load. Cancelling
/// the picker does nothing; only a genuinely-absent portal falls back to a
/// self-snapshot.
pub fn capture_screen<F: Fn(Option<Pixbuf>) + 'static>(fallback: &ApplicationWindow, done: F) {
    let fallback = fallback.clone();
    glib::spawn_future_local(async move {
        match request_portal_screenshot().await {
            Capture::Image(pb) => done(Some(pb)),
            Capture::Abort => done(None),
            Capture::NoPortal => {
                eprintln!("tcode: screenshot portal unavailable; capturing Tcode's own window");
                capture_window_async(&fallback, done)
            }
        }
    });
}

/// Run the interactive screenshot portal. The portal being unreachable (the
/// `send` fails) is the only case that warrants the self-snapshot fallback; once
/// the picker is up, any non-image outcome — cancel, denial, error — means
/// "abort", so a cancelled capture never opens the annotation canvas.
async fn request_portal_screenshot() -> Capture {
    use ashpd::desktop::screenshot::Screenshot;
    let request = match Screenshot::request().interactive(true).modal(true).send().await {
        Ok(r) => r,
        Err(_) => return Capture::NoPortal,
    };
    match request.response() {
        // `uri()` Displays as a `file://` URI; glib decodes it (percent-encoding
        // and all) to a path, no `url` crate needed.
        Ok(response) => glib::filename_from_uri(&response.uri().to_string())
            .ok()
            .and_then(|(path, _host)| Pixbuf::from_file(&path).ok())
            .map(Capture::Image)
            .unwrap_or(Capture::Abort),
        Err(_) => Capture::Abort,
    }
}

/// Snapshot the live Tcode window into a `Pixbuf`, asynchronously.
///
/// `WidgetPaintable` only records a render node on the widget's *next* snapshot
/// after it is attached — so a synchronous capture of an already-drawn, static
/// widget yields an empty node. Instead we attach the paintable, force a
/// redraw, and read back two frame-clock ticks later. A timeout backstop avoids
/// hanging if the frame clock is idle (e.g. the window is fully occluded).
pub fn capture_window_async<F: Fn(Option<Pixbuf>) + 'static>(window: &ApplicationWindow, done: F) {
    let target: gtk4::Widget = window
        .child()
        .unwrap_or_else(|| window.clone().upcast::<gtk4::Widget>());
    let renderer = match window.renderer() {
        Some(r) => r,
        None => {
            done(None);
            return;
        }
    };
    let paintable = gtk4::WidgetPaintable::new(Some(&target));
    target.queue_draw();

    let done = std::rc::Rc::new(std::cell::RefCell::new(Some(done)));
    let ticks = std::cell::Cell::new(0u32);
    {
        let done = done.clone();
        target.add_tick_callback(move |w, _clock| {
            let n = ticks.get() + 1;
            ticks.set(n);
            if n < 2 {
                return glib::ControlFlow::Continue;
            }
            let pb = capture_paintable(&paintable, &renderer, w);
            if let Some(cb) = done.borrow_mut().take() {
                cb(pb);
            }
            glib::ControlFlow::Break
        });
    }

    let done2 = done;
    glib::timeout_add_local_once(std::time::Duration::from_millis(1200), move || {
        if let Some(cb) = done2.borrow_mut().take() {
            cb(None);
        }
    });
}

/// Render the paintable's current node to a `Pixbuf` via the window renderer.
fn capture_paintable(
    paintable: &gtk4::WidgetPaintable,
    renderer: &gtk4::gsk::Renderer,
    w: &gtk4::Widget,
) -> Option<Pixbuf> {
    let (pw, ph) = (w.width(), w.height());
    if pw <= 0 || ph <= 0 {
        return None;
    }
    let snapshot = gtk4::Snapshot::new();
    paintable.snapshot(
        snapshot.upcast_ref::<gtk4::gdk::Snapshot>(),
        pw as f64,
        ph as f64,
    );
    let node = snapshot.to_node()?;
    let texture = renderer.render_texture(&node, None);
    // Encode to PNG in memory and decode straight back — no temp file on disk.
    // A predictable, world-readable /tmp path was both a screenshot-disclosure
    // and a symlink-clobber risk.
    let bytes = texture.save_to_png_bytes();
    Pixbuf::from_read(std::io::Cursor::new(bytes.to_vec())).ok()
}
