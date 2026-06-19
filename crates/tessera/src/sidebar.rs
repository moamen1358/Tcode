//! The left file panel: a VS Code-style expandable tree rooted at the launch
//! directory. Single-click a folder to expand/collapse it in place; single-click
//! a file to open it in the editor panel beside the terminals.

use std::path::Path;

use gtk4::prelude::*;
use gtk4::gio;
use gtk4::{
    Align, Box as GtkBox, CustomSorter, DirectoryList, EventControllerMotion, GestureClick, Image,
    Label, ListItem, ListView, NoSelection, Ordering, Orientation, PolicyType, ScrolledWindow,
    SignalListItemFactory, SortListModel, TreeExpander, TreeListModel, TreeListRow,
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
        Some("standard::name,standard::display-name,standard::type,standard::icon,standard::file"),
        Some(dir),
    )
}

/// Recover the `gio::File` for a row via the auto-set `standard::file` attribute.
fn file_of(info: &gio::FileInfo) -> Option<gio::File> {
    info.attribute_object("standard::file")
        .and_then(|obj| obj.downcast::<gio::File>().ok())
}

/// Recursively remove the custom hover class across a widget subtree.
fn clear_hovered(w: &impl IsA<gtk4::Widget>) {
    w.remove_css_class("hovered");
    let mut child = w.first_child();
    while let Some(c) = child {
        clear_hovered(&c);
        child = c.next_sibling();
    }
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

        // Clicking anywhere in the sidebar — including empty space below the
        // files — pulls keyboard focus off the terminal so its focus ring clears.
        root.set_focusable(true);
        let click = GestureClick::new();
        let root_weak = root.downgrade();
        click.connect_pressed(move |_, _, _, _| {
            if let Some(r) = root_weak.upgrade() {
                r.grab_focus();
            }
        });
        root.add_controller(click);

        // The launch directory itself is the tree root; everything else nests
        // under it (like VS Code's workspace folder).
        let root_file = gio::File::for_path(start);
        let root_name = start
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| start.display().to_string());
        let root_info = gio::FileInfo::new();
        root_info.set_file_type(gio::FileType::Directory);
        root_info.set_name(Path::new(&root_name));
        root_info.set_attribute_string("standard::display-name", &root_name);
        root_info.set_attribute_object("standard::file", &root_file);
        let root_store = gio::ListStore::new::<gio::FileInfo>();
        root_store.append(&root_info);

        // Lazy tree: each directory yields a sorted child listing; files are leaves.
        // TESSERA_AUTOEXPAND=1 expands every folder (visual-testing aid).
        let autoexpand = std::env::var_os("TESSERA_AUTOEXPAND").is_some();
        let tree = TreeListModel::new(root_store, false, autoexpand, |obj| {
            let info = obj.downcast_ref::<gio::FileInfo>()?;
            if info.file_type() == gio::FileType::Directory {
                let dir = file_of(info)?;
                Some(sorted_dir(&dir))
            } else {
                None
            }
        });
        // Expand the workspace root by default so its children are visible.
        if let Some(root_row) = tree.item(0).and_downcast::<TreeListRow>() {
            root_row.set_expanded(true);
        }
        // No-selection model: clicks still activate rows (open / expand), but no
        // row is ever marked selected, so nothing stays highlighted.
        let selection = NoSelection::new(Some(tree));

        let factory = SignalListItemFactory::new();
        factory.connect_setup(|_f, obj| {
            let Some(item) = obj.downcast_ref::<ListItem>() else {
                return;
            };
            // Row layout: [indent guides] [icon + label]. We hide the disclosure
            // triangle and draw our own indent guide lines, so set the expander to
            // neither show an arrow nor indent (we handle indentation ourselves).
            let indent = GtkBox::new(Orientation::Horizontal, 0);
            let content = GtkBox::new(Orientation::Horizontal, 6);
            let image = Image::new();
            image.set_pixel_size(14);
            let label = Label::builder().halign(Align::Start).build();
            content.append(&image);
            content.append(&label);
            let row = GtkBox::new(Orientation::Horizontal, 0);
            row.append(&indent);
            row.append(&content);
            let expander = TreeExpander::new();
            expander.set_hide_expander(true);
            expander.set_indent_for_depth(false);
            expander.set_child(Some(&row));

            // Custom hover highlight: a per-row motion controller toggles the
            // `.hovered` class on enter/leave. Unlike the native listview :hover,
            // this clears reliably when the pointer leaves the row.
            let hover = EventControllerMotion::new();
            let enter_w = expander.downgrade();
            hover.connect_enter(move |_, _, _| {
                if let Some(e) = enter_w.upgrade() {
                    e.add_css_class("hovered");
                }
            });
            let leave_w = expander.downgrade();
            hover.connect_leave(move |_| {
                if let Some(e) = leave_w.upgrade() {
                    e.remove_css_class("hovered");
                }
            });
            expander.add_controller(hover);

            item.set_child(Some(&expander));
        });
        let icons_dir = crate::icons::ensure();
        factory.connect_bind(move |_f, obj| {
            let Some(item) = obj.downcast_ref::<ListItem>() else {
                return;
            };
            let Some(treerow) = item.item().and_downcast::<TreeListRow>() else {
                return;
            };
            let Some(expander) = item.child().and_downcast::<TreeExpander>() else {
                return;
            };
            let Some(row) = expander.child().and_downcast::<GtkBox>() else {
                return;
            };
            let Some(indent) = row.first_child().and_downcast::<GtkBox>() else {
                return;
            };
            let Some(content) = row.last_child().and_downcast::<GtkBox>() else {
                return;
            };
            let Some(image) = content.first_child().and_downcast::<Image>() else {
                return;
            };
            let Some(label) = content.last_child().and_downcast::<Label>() else {
                return;
            };
            expander.set_list_row(Some(&treerow));

            // One vertical guide line per ancestor level (drawn left of the row).
            while let Some(child) = indent.first_child() {
                indent.remove(&child);
            }
            for _ in 0..treerow.depth() {
                let cell = GtkBox::new(Orientation::Horizontal, 0);
                cell.set_size_request(14, -1);
                cell.add_css_class("indent-guide");
                indent.append(&cell);
            }

            if let Some(info) = treerow.item().and_downcast::<gio::FileInfo>() {
                label.set_label(&info.display_name());
                // Per-type icon from the bundled Material set, rasterized by
                // librsvg at the exact device-pixel size (display scale, floored
                // at 2× so it stays crisp on HiDPI even before the row knows its
                // monitor) instead of letting GtkImage scale a natural-size bitmap.
                let is_dir = info.file_type() == gio::FileType::Directory;
                let name = info.name();
                let scale = image.scale_factor().max(2);
                match crate::icons::icon_texture(&icons_dir, &name.to_string_lossy(), is_dir, 14 * scale) {
                    Some(tex) => image.set_paintable(Some(&tex)),
                    None => image.clear(),
                }
            }
        });

        let list = ListView::new(Some(selection), Some(factory));
        list.set_single_click_activate(true);
        let st = state.clone();
        list.connect_activate(move |lv, pos| {
            let Some(model) = lv.model() else { return };
            let Some(row) = model.item(pos).and_downcast::<TreeListRow>() else {
                return;
            };
            let Some(info) = row.item().and_downcast::<gio::FileInfo>() else {
                return;
            };
            if info.file_type() == gio::FileType::Directory {
                row.set_expanded(!row.is_expanded());
            } else if let Some(path) = file_of(&info).and_then(|f| f.path()) {
                open_file(&st, &path);
            } else {
                eprintln!("tessera: could not resolve path for sidebar item");
            }
        });

        let scroller = ScrolledWindow::builder()
            .hscrollbar_policy(PolicyType::Automatic)
            .vexpand(true)
            .child(&list)
            .build();
        root.append(&scroller);

        // Safety net: if a row's hover-leave is ever missed, clear every hover
        // highlight once the pointer leaves the sidebar entirely.
        let leave = EventControllerMotion::new();
        let root_weak = root.downgrade();
        leave.connect_leave(move |_| {
            if let Some(root) = root_weak.upgrade() {
                clear_hovered(&root);
            }
        });
        root.add_controller(leave);

        Sidebar { root }
    }
}
