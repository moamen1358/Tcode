//! The left file panel: a navigable directory list (go in / go back with `..`),
//! VS Code-ish but navigation-first. Click a folder to `cd` the focused pane
//! into it (and descend); click a file to insert its path into the focused pane.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::{
    Box as GtkBox, Label, ListBox, ListBoxRow, Orientation, PolicyType, ScrolledWindow,
    SelectionMode,
};

use crate::app::Shared;

/// Single-quote a path for safe shell insertion (handles spaces and quotes).
pub(crate) fn shell_quote(p: &Path) -> String {
    // Strip control chars (ESC, CR, BEL, …) so a crafted filename can't inject
    // terminal escape sequences, then single-quote for the shell.
    let cleaned: String = p
        .to_string_lossy()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    format!("'{}'", cleaned.replace('\'', "'\\''"))
}

#[derive(Clone)]
enum Kind {
    Up,
    Dir,
    File,
}

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    kind: Kind,
}

pub struct Sidebar {
    pub root: GtkBox,
}

impl Sidebar {
    pub fn new(start: &Path, state: &Shared) -> Sidebar {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("sidebar");
        root.set_width_request(240);

        let header = Label::new(None);
        header.add_css_class("sidebar-header");
        header.set_xalign(0.0);
        header.set_ellipsize(EllipsizeMode::Start);
        root.append(&header);

        let list = ListBox::new();
        list.set_selection_mode(SelectionMode::None);
        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        root.append(&scroller);

        let cwd = Rc::new(RefCell::new(start.to_path_buf()));
        let entries: Rc<RefCell<Vec<Entry>>> = Rc::new(RefCell::new(Vec::new()));

        populate(&list, &header, &cwd.borrow(), &entries);

        let st = state.clone();
        let cwd_c = cwd.clone();
        let entries_c = entries.clone();
        let header_c = header.clone();
        let list_c = list.clone();
        list.connect_row_activated(move |_lb, row| {
            let idx = row.index();
            if idx < 0 {
                return;
            }
            // Copy the entry out, releasing the borrow before we repopulate.
            let entry = {
                let e = entries_c.borrow();
                match e.get(idx as usize) {
                    Some(x) => x.clone(),
                    None => return,
                }
            };
            match entry.kind {
                Kind::Up => {
                    *cwd_c.borrow_mut() = entry.path.clone();
                    populate(&list_c, &header_c, &cwd_c.borrow(), &entries_c);
                }
                Kind::Dir => {
                    feed_focused(&st, &format!("cd {}\n", shell_quote(&entry.path)));
                    *cwd_c.borrow_mut() = entry.path.clone();
                    populate(&list_c, &header_c, &cwd_c.borrow(), &entries_c);
                }
                Kind::File => {
                    feed_focused(&st, &shell_quote(&entry.path));
                }
            }
        });

        Sidebar { root }
    }
}

fn feed_focused(state: &Shared, text: &str) {
    if let Some(g) = state.borrow().grid.as_ref() {
        g.feed_focused(text);
    }
}

fn populate(list: &ListBox, header: &Label, cwd: &Path, entries: &RefCell<Vec<Entry>>) {
    while let Some(child) = list.first_child() {
        list.remove(&child);
    }
    header.set_text(&cwd.display().to_string());

    let mut ents: Vec<Entry> = Vec::new();
    if let Some(parent) = cwd.parent() {
        add_row(list, "⬆  ..", "row-dir");
        ents.push(Entry {
            path: parent.to_path_buf(),
            kind: Kind::Up,
        });
    }

    let mut dirs: Vec<(String, PathBuf)> = Vec::new();
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(cwd) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue; // skip dotfiles
            }
            let p = e.path();
            if p.is_dir() {
                dirs.push((name, p));
            } else {
                files.push((name, p));
            }
        }
    }
    dirs.sort_by_key(|(n, _)| n.to_lowercase());
    files.sort_by_key(|(n, _)| n.to_lowercase());

    for (name, p) in dirs {
        add_row(list, &format!("📁  {name}"), "row-dir");
        ents.push(Entry {
            path: p,
            kind: Kind::Dir,
        });
    }
    for (name, p) in files {
        add_row(list, &format!("📄  {name}"), "row-file");
        ents.push(Entry {
            path: p,
            kind: Kind::File,
        });
    }
    *entries.borrow_mut() = ents;
}

fn add_row(list: &ListBox, text: &str, css: &str) {
    let label = Label::new(Some(text));
    label.set_xalign(0.0);
    label.add_css_class(css);
    let row = ListBoxRow::new();
    row.set_child(Some(&label));
    list.append(&row);
}
