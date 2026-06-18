//! The left file panel: a VS Code-style expandable tree rooted at the launch
//! directory. Single-click a folder to expand/collapse it in place; single-click
//! a file to open it in the editor panel beside the terminals.

use std::path::Path;

use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::gio;
use gtk4::{
    Align, Box as GtkBox, CustomSorter, DirectoryList, Image, Label, ListItem, ListView, Ordering,
    Orientation, PolicyType, ScrolledWindow, SignalListItemFactory, SortListModel, SingleSelection,
    TreeExpander, TreeListModel, TreeListRow,
};

use crate::app::{open_file, Shared};

/// Single-quote a path for safe shell insertion (used by the image drop target).
pub(crate) fn shell_quote(p: &Path) -> String {
    let cleaned: String = p
        .to_string_lossy()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    format!("'{}'", cleaned.replace('\'', "'\\''"))
}

pub struct Sidebar {
    pub root: GtkBox,
}

fn directory_list_for(dir: &gio::File) -> DirectoryList {
    DirectoryList::new(
        Some("standard::name,standard::display-name,standard::type,standard::icon"),
        Some(dir),
    )
}

/// Recover the `gio::File` for a row via the auto-set `standard::file` attribute.
fn file_of(info: &gio::FileInfo) -> Option<gio::File> {
    info.attribute_object("standard::file")
        .and_then(|obj| obj.downcast::<gio::File>().ok())
}

/// A directory listing sorted Zed-style: folders first, then alphabetical.
fn sorted_dir(dir: &gio::File) -> gio::ListModel {
    let list = directory_list_for(dir);
    let sorter = CustomSorter::new(|a, b| {
        let (a, b) = match (
            a.downcast_ref::<gio::FileInfo>(),
            b.downcast_ref::<gio::FileInfo>(),
        ) {
            (Some(a), Some(b)) => (a, b),
            _ => return Ordering::Equal,
        };
        let adir = a.file_type() == gio::FileType::Directory;
        let bdir = b.file_type() == gio::FileType::Directory;
        if adir != bdir {
            return if adir { Ordering::Smaller } else { Ordering::Larger };
        }
        match a
            .display_name()
            .to_lowercase()
            .cmp(&b.display_name().to_lowercase())
        {
            std::cmp::Ordering::Less => Ordering::Smaller,
            std::cmp::Ordering::Greater => Ordering::Larger,
            std::cmp::Ordering::Equal => Ordering::Equal,
        }
    });
    SortListModel::new(Some(list), Some(sorter)).upcast()
}

impl Sidebar {
    pub fn new(start: &Path, state: &Shared) -> Sidebar {
        let root = GtkBox::new(Orientation::Vertical, 0);
        root.add_css_class("sidebar");
        root.set_width_request(120);

        let header_text = start
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| start.display().to_string());
        let header = Label::new(Some(&header_text));
        header.add_css_class("sidebar-header");
        header.set_xalign(0.0);
        header.set_ellipsize(EllipsizeMode::End);
        root.append(&header);

        // Lazy tree: each directory yields a child DirectoryList; files are leaves.
        let root_file = gio::File::for_path(start);
        let tree = TreeListModel::new(sorted_dir(&root_file), false, false, |obj| {
            let info = obj.downcast_ref::<gio::FileInfo>()?;
            if info.file_type() == gio::FileType::Directory {
                let dir = file_of(info)?;
                Some(sorted_dir(&dir))
            } else {
                None
            }
        });
        let selection = SingleSelection::new(Some(tree));

        let factory = SignalListItemFactory::new();
        factory.connect_setup(|_f, obj| {
            let item = obj.downcast_ref::<ListItem>().expect("ListItem");
            let row = GtkBox::new(Orientation::Horizontal, 6);
            let image = Image::new();
            image.set_pixel_size(16);
            let label = Label::builder().halign(Align::Start).build();
            row.append(&image);
            row.append(&label);
            let expander = TreeExpander::new();
            expander.set_child(Some(&row));
            item.set_child(Some(&expander));
        });
        let icons_dir = crate::icons::ensure();
        factory.connect_bind(move |_f, obj| {
            let item = obj.downcast_ref::<ListItem>().expect("ListItem");
            let Some(treerow) = item.item().and_downcast::<TreeListRow>() else {
                return;
            };
            let Some(expander) = item.child().and_downcast::<TreeExpander>() else {
                return;
            };
            let Some(row) = expander.child().and_downcast::<GtkBox>() else {
                return;
            };
            let Some(image) = row.first_child().and_downcast::<Image>() else {
                return;
            };
            let Some(label) = row.last_child().and_downcast::<Label>() else {
                return;
            };
            expander.set_list_row(Some(&treerow)); // disclosure triangle + indentation
            if let Some(info) = treerow.item().and_downcast::<gio::FileInfo>() {
                label.set_label(&info.display_name());
                // Colored per-type icon from the bundled set (Zed-style).
                let is_dir = info.file_type() == gio::FileType::Directory;
                let name = info.name();
                let path = crate::icons::icon_path(&icons_dir, &name.to_string_lossy(), is_dir);
                image.set_from_file(Some(&path));
            }
        });

        let list = ListView::new(Some(selection), Some(factory));
        list.set_single_click_activate(true);
        let st = state.clone();
        list.connect_activate(move |lv, pos| {
            let Some(model) = lv.model() else { return };
            let Ok(sel) = model.downcast::<SingleSelection>() else {
                return;
            };
            let Some(row) = sel.item(pos).and_downcast::<TreeListRow>() else {
                return;
            };
            let Some(info) = row.item().and_downcast::<gio::FileInfo>() else {
                return;
            };
            if info.file_type() == gio::FileType::Directory {
                row.set_expanded(!row.is_expanded());
            } else if let Some(path) = file_of(&info).and_then(|f| f.path()) {
                open_file(&st, &path);
            }
        });

        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Automatic)
            .vexpand(true)
            .child(&list)
            .build();
        root.append(&scroller);

        Sidebar { root }
    }
}
