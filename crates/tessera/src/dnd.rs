//! File drag-and-drop helpers shared by terminals and BridgeShot thumbnails.

use std::path::{Path, PathBuf};

use gtk4::gdk::{ContentProvider, DragAction, FileList};
use gtk4::prelude::*;
use gtk4::{gio, glib, DropTarget};

/// Single-quote a path for safe shell insertion.
pub(crate) fn shell_quote(p: &Path) -> String {
    let cleaned: String = p
        .to_string_lossy()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    format!("'{}'", cleaned.replace('\'', "'\\''"))
}

/// Cap on paths accepted from a single drop, so one drop can't flood the shell.
const MAX_DROP_PATHS: usize = 100;

pub(crate) fn shell_join_paths(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| shell_quote(p))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Accept files from file managers and text/URI fallbacks from other drag sources.
pub(crate) fn install_path_drop(
    widget: &impl IsA<gtk4::Widget>,
    on_paths: impl Fn(Vec<PathBuf>) + 'static,
) {
    let drop = DropTarget::new(glib::Type::INVALID, DragAction::COPY);
    drop.set_types(&[
        FileList::static_type(),
        gio::File::static_type(),
        String::static_type(),
    ]);
    drop.connect_drop(move |_t, value, _x, _y| {
        let Some(paths) = paths_from_drop_value(value) else {
            return false;
        };
        if paths.is_empty() {
            return false;
        }
        on_paths(paths);
        true
    });
    widget.add_controller(drop);
}

/// Provide both native GTK file data and common cross-app fallbacks.
pub(crate) fn file_drag_provider(path: &Path) -> ContentProvider {
    let file = gio::File::for_path(path);
    let file_list = FileList::from_array(std::slice::from_ref(&file));
    let uri = file.uri();
    let plain = path.to_string_lossy().to_string();
    let uri_list = format!("{uri}\r\n");
    ContentProvider::new_union(&[
        ContentProvider::for_value(&file_list.to_value()),
        ContentProvider::for_value(&file.to_value()),
        ContentProvider::for_value(&plain.to_value()),
        ContentProvider::for_bytes("text/uri-list", &glib::Bytes::from(uri_list.as_bytes())),
        ContentProvider::for_bytes("text/plain", &glib::Bytes::from(plain.as_bytes())),
    ])
}

fn paths_from_drop_value(value: &glib::Value) -> Option<Vec<PathBuf>> {
    if let Ok(list) = value.get::<FileList>() {
        return Some(
            list.files()
                .into_iter()
                .filter_map(|f| f.path())
                .take(MAX_DROP_PATHS)
                .collect(),
        );
    }
    if let Ok(file) = value.get::<gio::File>() {
        return Some(file.path().into_iter().collect());
    }
    if let Ok(text) = value.get::<String>() {
        let paths = paths_from_text(&text);
        if !paths.is_empty() {
            return Some(paths);
        }
    }
    None
}

fn paths_from_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#') && line.len() <= 4096)
        .filter_map(|line| {
            let p = if line.starts_with("file://") {
                gio::File::for_uri(line).path()?
            } else {
                PathBuf::from(line)
            };
            // Only accept text that names a real local path — don't turn arbitrary
            // dropped text into a bogus "path" fed to the shell.
            p.exists().then_some(p)
        })
        .take(MAX_DROP_PATHS)
        .collect()
}
