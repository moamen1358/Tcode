//! Off-main-thread file loading for the text/CSV viewers: a cheap binary sniff
//! plus a worker-thread full read, so opening a large file never freezes the UI.

use std::io::Read;
use std::path::Path;

use gtk4::glib;

/// Files larger than this are not loaded into a viewer (would freeze / OOM); the
/// caller shows the info card instead.
pub const MAX_TEXT_BYTES: u64 = 64 * 1024 * 1024;

/// Whether `path` exceeds the load cap.
pub fn too_large(path: &Path) -> bool {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_TEXT_BYTES
}

/// Sniff up to 8 KiB for a NUL byte — a cheap "is this binary?" test that avoids
/// reading the whole file, so a binary file can fall back to the info card
/// without blocking the UI on a full read.
pub fn looks_binary(path: &Path) -> bool {
    let mut buf = [0u8; 8192];
    match std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)) {
        Ok(n) => buf[..n].contains(&0),
        Err(_) => false,
    }
}

/// Read `path` fully on a worker thread, then deliver the decoded UTF-8 text (or
/// `None` for a read error / non-UTF-8) to `on_done` on the GTK main thread.
pub fn load_text_async(path: &Path, on_done: impl FnOnce(Option<String>) + 'static) {
    let path = path.to_path_buf();
    let (tx, rx) = async_channel::bounded::<Option<String>>(1);
    std::thread::spawn(move || {
        let text = std::fs::read(&path)
            .ok()
            .and_then(|b| String::from_utf8(b).ok());
        let _ = tx.send_blocking(text);
    });
    glib::spawn_future_local(async move {
        if let Ok(text) = rx.recv().await {
            on_done(text);
        }
    });
}
