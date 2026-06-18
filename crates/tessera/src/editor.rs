//! Tabbed universal file viewer beside the terminals (the center `Paned`'s end
//! child). Each opened file becomes a tab whose content is chosen by file type:
//! text/code uses GtkSourceView (editable, `Ctrl+S` saves); images use
//! GtkPicture; PDFs and office documents are rendered to page images (see
//! `preview`); anything else gets an info card with "Open externally". `Esc` or a
//! tab's `×` closes it; closing the last tab hides the panel.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk::{Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, ContentFit, EventControllerKey, Label, Notebook, Orientation,
    Paned, Picture, PolicyType, PropagationPhase, ScrolledWindow, Widget,
};
use sourceview5::prelude::*;
use sourceview5::{Buffer, LanguageManager, StyleSchemeManager, View};

use crate::preview;

/// What kind of viewer a file maps to (by extension).
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Text,
    Image,
    Pdf,
    Office,
    Other,
}

/// Pick a viewer kind from the file name/extension.
pub fn kind_of(path: &Path) -> Kind {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "svg" | "tif" | "tiff"
        | "xpm" | "pnm" | "tga" | "icns" => Kind::Image,
        "pdf" => Kind::Pdf,
        // Office formats (rendered via soffice -> pdf -> images). CSV is left as
        // text since users typically edit it.
        "doc" | "docx" | "odt" | "rtf" | "ppt" | "pptx" | "odp" | "xls" | "xlsx" | "ods" => {
            Kind::Office
        }
        // Media / archives / binaries get the info card (no inline preview).
        "mp4" | "mkv" | "webm" | "mov" | "avi" | "wmv" | "flv" | "mp3" | "wav" | "flac"
        | "ogg" | "m4a" | "aac" | "opus" | "zip" | "tar" | "gz" | "xz" | "zst" | "bz2"
        | "7z" | "rar" | "exe" | "bin" | "so" | "o" | "a" | "dll" | "dylib" | "wasm"
        | "ttf" | "otf" | "woff" | "woff2" => Kind::Other,
        // Unknown/extensionless default to text, downgraded to the info card if
        // the bytes aren't valid UTF-8.
        _ => Kind::Text,
    }
}

struct OpenFile {
    path: PathBuf,
    /// `Some` only for editable text tabs (so `Ctrl+S` knows what it can save).
    buffer: Option<Buffer>,
    /// The tab's content widget (its identity in the notebook).
    child: Widget,
}

type OpenFiles = Rc<RefCell<Vec<OpenFile>>>;

pub struct Editor {
    pub root: Notebook,
    paned: Paned,
    open: OpenFiles,
}

impl Editor {
    pub fn new(paned: &Paned) -> Editor {
        let notebook = Notebook::new();
        notebook.set_scrollable(true);
        notebook.add_css_class("editor");

        let open: OpenFiles = Rc::new(RefCell::new(Vec::new()));

        // Ctrl+S saves the current text tab; Esc closes the current tab.
        {
            let nb = notebook.clone();
            let open_c = open.clone();
            let paned_c = paned.clone();
            let kc = EventControllerKey::new();
            kc.set_propagation_phase(PropagationPhase::Capture);
            kc.connect_key_pressed(move |_c, key, _code, mods| {
                if mods.contains(ModifierType::CONTROL_MASK) && key == Key::s {
                    save_current(&nb, &open_c);
                    Propagation::Stop
                } else if key == Key::Escape {
                    if let Some(cur) = nb.current_page() {
                        let child = open_c
                            .borrow()
                            .iter()
                            .find(|of| nb.page_num(&of.child) == Some(cur))
                            .map(|of| of.child.clone());
                        if let Some(child) = child {
                            close_tab(&nb, &open_c, &paned_c, &child);
                        }
                    }
                    Propagation::Stop
                } else {
                    Propagation::Proceed
                }
            });
            notebook.add_controller(kc);
        }

        Editor {
            root: notebook,
            paned: paned.clone(),
            open,
        }
    }

    /// Open `path` in a tab (focusing it if already open) and reveal the panel.
    pub fn open(&self, path: &Path) {
        if let Some(child) = self
            .open
            .borrow()
            .iter()
            .find(|of| of.path == path)
            .map(|of| of.child.clone())
        {
            if let Some(p) = self.root.page_num(&child) {
                self.root.set_current_page(Some(p));
            }
            self.reveal();
            return;
        }

        let (child, buffer) = build_viewer(path);

        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.display().to_string());
        let tab = GtkBox::new(Orientation::Horizontal, 6);
        tab.add_css_class("editor-tab");
        let label = Label::new(Some(&name));
        let close = Button::from_icon_name("window-close-symbolic");
        close.add_css_class("flat");
        tab.append(&label);
        tab.append(&close);

        let page = self.root.append_page(&child, Some(&tab));
        self.root.set_current_page(Some(page));
        self.open.borrow_mut().push(OpenFile {
            path: path.to_path_buf(),
            buffer,
            child: child.clone(),
        });

        {
            let nb = self.root.clone();
            let open_c = self.open.clone();
            let paned_c = self.paned.clone();
            let child = child.clone();
            close.connect_clicked(move |_| close_tab(&nb, &open_c, &paned_c, &child));
        }

        self.reveal();
        child.grab_focus();
    }

    fn reveal(&self) {
        if self.root.parent().is_none() {
            self.paned.set_end_child(Some(&self.root));
        }
        self.root.set_visible(true);
        let w = self.paned.width();
        self.paned.set_position(if w > 200 { w / 2 } else { 700 });
    }
}

/// Build the tab content for `path` based on its kind. Returns the widget and,
/// for editable text, its source buffer.
fn build_viewer(path: &Path) -> (Widget, Option<Buffer>) {
    match kind_of(path) {
        Kind::Text => match text_viewer(path) {
            Some((w, b)) => (w, Some(b)),
            None => (fallback_viewer(path), None), // binary masquerading as text
        },
        Kind::Image => (image_viewer(path), None),
        Kind::Pdf | Kind::Office => (preview::document_viewer(path), None),
        Kind::Other => (fallback_viewer(path), None),
    }
}

/// Editable source view with line numbers + syntax highlighting. Returns `None`
/// if the file isn't valid UTF-8 (binary) so the caller can show the info card.
fn text_viewer(path: &Path) -> Option<(Widget, Buffer)> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None; // NUL byte -> binary
    }
    let content = String::from_utf8(bytes).ok()?;

    let buffer = Buffer::new(None);
    if let Some(lang) = LanguageManager::default().guess_language(path.to_str(), None) {
        buffer.set_language(Some(&lang));
    }
    if let Some(scheme) = StyleSchemeManager::default().scheme("Adwaita-dark") {
        buffer.set_style_scheme(Some(&scheme));
    }
    buffer.set_text(&content);

    let view = View::with_buffer(&buffer);
    view.set_show_line_numbers(true);
    view.set_highlight_current_line(true);
    view.set_left_margin(14);
    view.add_css_class("editor-view");
    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&view)
        .build();
    Some((scroller.upcast(), buffer))
}

/// Fit-to-view image, scrollable for very large pictures.
fn image_viewer(path: &Path) -> Widget {
    let pic = Picture::for_filename(path);
    pic.set_content_fit(ContentFit::Contain);
    pic.set_can_shrink(true);
    pic.set_vexpand(true);
    pic.set_hexpand(true);
    pic.add_css_class("image-view");
    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&pic)
        .build();
    scroller.upcast()
}

/// Info card for files we can't preview inline (video, audio, archives, …).
fn fallback_viewer(path: &Path) -> Widget {
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let size = std::fs::metadata(path)
        .map(|m| human_size(m.len()))
        .unwrap_or_else(|_| "?".into());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_uppercase())
        .unwrap_or_else(|| "FILE".into());

    let card = GtkBox::new(Orientation::Vertical, 10);
    card.set_halign(Align::Center);
    card.set_valign(Align::Center);
    card.add_css_class("fallback-card");

    let title = Label::new(Some(&name));
    title.add_css_class("fallback-title");
    let meta = Label::new(Some(&format!("{ext} · {size}")));
    meta.add_css_class("fallback-meta");
    let no_preview = Label::new(Some("No inline preview for this file type"));
    no_preview.add_css_class("fallback-meta");

    let open = Button::with_label("Open externally");
    open.add_css_class("fallback-open");
    let p = path.to_path_buf();
    open.connect_clicked(move |_| {
        let _ = std::process::Command::new("xdg-open").arg(&p).spawn();
    });

    card.append(&title);
    card.append(&meta);
    card.append(&no_preview);
    card.append(&open);
    card.upcast()
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn save_current(nb: &Notebook, open: &OpenFiles) {
    let Some(cur) = nb.current_page() else { return };
    let files = open.borrow();
    if let Some(of) = files.iter().find(|of| nb.page_num(&of.child) == Some(cur)) {
        if let Some(b) = &of.buffer {
            let text = b.text(&b.start_iter(), &b.end_iter(), false);
            if let Err(e) = std::fs::write(&of.path, text.as_str()) {
                eprintln!("tessera: save failed for {}: {e}", of.path.display());
            }
        }
    }
}

fn close_tab(nb: &Notebook, open: &OpenFiles, paned: &Paned, child: &Widget) {
    if let Some(p) = nb.page_num(child) {
        nb.remove_page(Some(p));
    }
    open.borrow_mut().retain(|of| &of.child != child);
    if open.borrow().is_empty() {
        paned.set_end_child(None::<&Widget>);
    }
}
