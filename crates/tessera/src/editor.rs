//! A tabbed, neovim-style file editor that opens beside the terminals (the
//! center `Paned`'s end child). Built on GtkSourceView: line numbers,
//! current-line highlight, and syntax highlighting (language guessed from the
//! filename). Open many files as tabs; `Ctrl+S` saves the active tab, `Esc` or a
//! tab's `×` closes it; closing the last tab hides the panel.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gdk::{Key, ModifierType};
use gtk4::glib::Propagation;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Button, EventControllerKey, Label, Notebook, Orientation, Paned, PolicyType,
    PropagationPhase, ScrolledWindow,
};
use sourceview5::prelude::*;
use sourceview5::{Buffer, LanguageManager, StyleSchemeManager, View};

struct OpenFile {
    path: PathBuf,
    buffer: Buffer,
    child: ScrolledWindow,
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

        // Ctrl+S saves the current tab; Esc closes it (editor subtree only).
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

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => format!("[tessera] cannot open {}: {e}", path.display()),
        };

        let buffer = Buffer::new(None);
        // Syntax highlighting: guess the language from the filename.
        if let Some(lang) = LanguageManager::default().guess_language(path.to_str(), None) {
            buffer.set_language(Some(&lang));
        }
        // Dark style scheme for the syntax colors.
        if let Some(scheme) = StyleSchemeManager::default().scheme("Adwaita-dark") {
            buffer.set_style_scheme(Some(&scheme));
        }
        buffer.set_text(&content);

        let view = View::with_buffer(&buffer);
        view.set_show_line_numbers(true);
        view.set_highlight_current_line(true);
        view.add_css_class("editor-view"); // font via CSS (theme)
        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Automatic)
            .vexpand(true)
            .hexpand(true)
            .child(&view)
            .build();

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

        let page = self.root.append_page(&scroller, Some(&tab));
        self.root.set_current_page(Some(page));
        self.open.borrow_mut().push(OpenFile {
            path: path.to_path_buf(),
            buffer,
            child: scroller.clone(),
        });

        {
            let nb = self.root.clone();
            let open_c = self.open.clone();
            let paned_c = self.paned.clone();
            let child = scroller.clone();
            close.connect_clicked(move |_| close_tab(&nb, &open_c, &paned_c, &child));
        }

        self.reveal();
        view.grab_focus();
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

fn save_current(nb: &Notebook, open: &OpenFiles) {
    let Some(cur) = nb.current_page() else { return };
    let files = open.borrow();
    if let Some(of) = files.iter().find(|of| nb.page_num(&of.child) == Some(cur)) {
        let b = &of.buffer;
        let text = b.text(&b.start_iter(), &b.end_iter(), false);
        if let Err(e) = std::fs::write(&of.path, text.as_str()) {
            eprintln!("tessera: save failed for {}: {e}", of.path.display());
        }
    }
}

fn close_tab(nb: &Notebook, open: &OpenFiles, paned: &Paned, child: &ScrolledWindow) {
    if let Some(p) = nb.page_num(child) {
        nb.remove_page(Some(p));
    }
    open.borrow_mut().retain(|of| &of.child != child);
    if open.borrow().is_empty() {
        paned.set_end_child(None::<&gtk4::Widget>);
    }
}
