//! Image capture for BridgeShot: the XDG screenshot portal (any window/region,
//! Wayland-safe) with the Tessera self-snapshot as a fallback.

use gtk4::gdk_pixbuf::Pixbuf;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::ApplicationWindow;
use std::sync::atomic::{AtomicU64, Ordering};

static CAPTURE_SEQ: AtomicU64 = AtomicU64::new(0);

/// Capture ANY window/region via the desktop portal. COSMIC shows its own
/// interactive picker; the chosen area comes back as a PNG we load. On any
/// failure (portal absent/denied/cancelled) we fall back to snapshotting
/// Tessera's own window so capture never dead-ends.
pub fn capture_screen<F: Fn(Option<Pixbuf>) + 'static>(fallback: &ApplicationWindow, done: F) {
    let fallback = fallback.clone();
    glib::spawn_future_local(async move {
        match request_portal_screenshot().await {
            Some(pb) => done(Some(pb)),
            None => {
                eprintln!("tessera: screenshot portal unavailable; capturing Tessera's own window");
                capture_window_async(&fallback, done)
            }
        }
    });
}

/// Returns the captured Pixbuf, or None if the portal failed/was cancelled.
async fn request_portal_screenshot() -> Option<Pixbuf> {
    use ashpd::desktop::screenshot::Screenshot;
    let response = Screenshot::request()
        .interactive(true)
        .modal(true)
        .send()
        .await
        .ok()?
        .response()
        .ok()?;
    // `uri()` Displays as a `file://` URI; glib decodes it to a path (handles
    // percent-encoding) without us depending on the url crate's API surface.
    let uri = response.uri().to_string();
    let (path, _host) = glib::filename_from_uri(&uri).ok()?;
    Pixbuf::from_file(&path).ok()
}

/// Snapshot the live Tessera window into a `Pixbuf`, asynchronously.
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
    let tmp = std::env::temp_dir().join(format!(
        "tessera-bridgeshot-capture-{}-{}.png",
        std::process::id(),
        CAPTURE_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    if let Err(e) = texture.save_to_png(&tmp) {
        eprintln!("tessera: snapshot save failed: {e}");
        return None;
    }
    let pb = Pixbuf::from_file(&tmp).ok();
    let _ = std::fs::remove_file(&tmp);
    pb
}
