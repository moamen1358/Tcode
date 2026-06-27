//! The left file panel: a VS Code-style expandable tree rooted at the launch
//! directory. Single-click a folder to expand/collapse it in place; single-click
//! a file to open it in the editor panel beside the terminals.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gtk4::gio;
use gtk4::glib;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::{
    Align, Box as GtkBox, Button, CustomSorter, DirectoryList, EventControllerMotion, GestureClick,
    Image, Label, ListItem, ListView, NoSelection, Ordering, Orientation, PolicyType,
    ScrolledWindow, Separator, SignalListItemFactory, SortListModel, TreeExpander, TreeListModel,
    TreeListRow,
};

use crate::app::{open_file, Shared};

#[derive(Clone)]
pub struct Sidebar {
    pub root: GtkBox,
}

fn directory_list_for(dir: &gio::File) -> DirectoryList {
    DirectoryList::new(
        Some(
            "standard::name,standard::display-name,standard::type,standard::icon,standard::file,standard::is-symlink",
        ),
        Some(dir),
    )
}

/// Recover the `gio::File` for a row via the auto-set `standard::file` attribute.
fn file_of(info: &gio::FileInfo) -> Option<gio::File> {
    info.attribute_object("standard::file")
        .and_then(|obj| obj.downcast::<gio::File>().ok())
}

/// Remove the custom hover class across a widget subtree. Iterative (an explicit
/// stack) so a deeply nested tree can't overflow the call stack.
fn clear_hovered(w: &impl IsA<gtk4::Widget>) {
    let mut stack = vec![w.clone().upcast::<gtk4::Widget>()];
    while let Some(n) = stack.pop() {
        n.remove_css_class("hovered");
        let mut c = n.first_child();
        while let Some(child) = c {
            c = child.next_sibling();
            stack.push(child);
        }
    }
}

fn set_css_class(w: &impl IsA<gtk4::Widget>, class: &str, enabled: bool) {
    if enabled {
        w.add_css_class(class);
    } else {
        w.remove_css_class(class);
    }
}

fn file_icon_for(expander: &TreeExpander) -> Option<Image> {
    let row = expander.child().and_downcast::<GtkBox>()?;
    let content = row.last_child().and_downcast::<GtkBox>()?;
    let disclosure = content.first_child().and_downcast::<Image>()?;
    disclosure.next_sibling().and_downcast::<Image>()
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
            return if adir {
                Ordering::Smaller
            } else {
                Ordering::Larger
            };
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
        // TCODE_AUTOEXPAND=1 expands every folder (visual-testing aid).
        let autoexpand = std::env::var_os("TCODE_AUTOEXPAND").is_some();
        let tree = TreeListModel::new(root_store, false, autoexpand, move |obj| {
            let info = obj.downcast_ref::<gio::FileInfo>()?;
            // Descend into directories, symlinked ones included, so they're
            // navigable like real folders. Exception: under TCODE_AUTOEXPAND (a
            // test aid that expands every row) don't follow a symlinked dir — a
            // symlink cycle would auto-expand without end. Manual expansion is
            // one level per click, so a cycle can't run away there.
            if info.file_type() == gio::FileType::Directory && !(autoexpand && info.is_symlink()) {
                let dir = file_of(info)?;
                Some(sorted_dir(&dir))
            } else {
                None
            }
        });
        // Expand the workspace root by default so its children are visible.
        let root_tree_row = tree.item(0).and_downcast::<TreeListRow>();
        if let Some(root_row) = root_tree_row.as_ref() {
            root_row.set_expanded(true);
        }
        let root_disclosure: Rc<RefCell<Option<glib::WeakRef<Image>>>> =
            Rc::new(RefCell::new(None));

        // Compact context label from the selected "Terminal Native" direction.
        // The project root remains a real expandable row immediately below it;
        // this header only improves hierarchy and never duplicates tree behavior.
        let header = GtkBox::new(Orientation::Horizontal, 8);
        header.add_css_class("sidebar-tree-header");
        let files_label = Label::new(Some("FILES"));
        let slash_label = Label::new(Some("/"));
        slash_label.add_css_class("header-slash");
        let project_label = Label::new(Some(&root_name.to_uppercase()));
        project_label.set_ellipsize(EllipsizeMode::End);
        project_label.set_hexpand(true);
        project_label.set_xalign(0.0);
        let header_toggle = Button::from_icon_name("pan-down-symbolic");
        header_toggle.add_css_class("sidebar-header-toggle");
        header_toggle.set_tooltip_text(Some("Collapse or expand project files"));
        if let Some(root_row) = root_tree_row {
            let root_disclosure = root_disclosure.clone();
            header_toggle.connect_clicked(move |button| {
                let expanded = !root_row.is_expanded();
                root_row.set_expanded(expanded);
                let icon = if expanded {
                    "pan-down-symbolic"
                } else {
                    "pan-end-symbolic"
                };
                button.set_icon_name(icon);
                if let Some(disclosure) = root_disclosure
                    .borrow()
                    .as_ref()
                    .and_then(glib::WeakRef::upgrade)
                {
                    disclosure.set_icon_name(Some(icon));
                }
            });
        }
        header.append(&files_label);
        header.append(&slash_label);
        header.append(&project_label);
        header.append(&header_toggle);
        root.append(&header);

        // No-selection model: clicks still activate rows (open / expand), but no
        // native list row is marked selected. The currently opened file receives
        // the custom orange focus outline without folder clicks clearing it.
        let selection = NoSelection::new(Some(tree));

        // Visible row handles let activation refresh the active-file outline and
        // a folder's disclosure icon immediately. Weak refs keep recycled ListView
        // rows from being retained after they leave the viewport.
        // TCODE_ACTIVE_FILE is a visual-test aid mirroring TCODE_AUTOEXPAND; normal
        // sessions start with no outlined row until the user opens a file here.
        let initial_active = std::env::var_os("TCODE_ACTIVE_FILE")
            .map(PathBuf::from)
            .map(|path| {
                if path.is_absolute() {
                    path
                } else {
                    start.join(path)
                }
            })
            .filter(|path| path.is_file());
        let active_file: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(initial_active));
        let visible_rows: Rc<RefCell<HashMap<PathBuf, glib::WeakRef<TreeExpander>>>> =
            Rc::new(RefCell::new(HashMap::new()));
        let visible_disclosures: Rc<RefCell<HashMap<PathBuf, glib::WeakRef<Image>>>> =
            Rc::new(RefCell::new(HashMap::new()));

        let factory = SignalListItemFactory::new();
        factory.connect_setup(|_f, obj| {
            let Some(item) = obj.downcast_ref::<ListItem>() else {
                return;
            };
            // Row layout: [indent guides] [branch] [disclosure] [icon] [label].
            // A real separator draws the horizontal branch; the disclosure uses
            // GTK's symbolic chevrons rather than text-glyph approximations.
            let indent = GtkBox::new(Orientation::Horizontal, 0);
            let branch = Separator::new(Orientation::Horizontal);
            branch.add_css_class("tree-branch");
            branch.set_size_request(8, -1);
            branch.set_valign(Align::Center);
            let content = GtkBox::new(Orientation::Horizontal, 5);
            content.set_hexpand(true);
            let disclosure = Image::new();
            disclosure.set_pixel_size(14);
            disclosure.set_size_request(14, 14);
            disclosure.add_css_class("tree-disclosure");
            let image = Image::new();
            image.set_pixel_size(14);
            let label = Label::builder().halign(Align::Start).build();
            label.set_ellipsize(EllipsizeMode::End);
            label.set_hexpand(true);
            label.set_xalign(0.0);
            content.append(&disclosure);
            content.append(&image);
            content.append(&label);
            let row = GtkBox::new(Orientation::Horizontal, 0);
            row.set_hexpand(true);
            row.append(&indent);
            row.append(&branch);
            row.append(&content);
            let expander = TreeExpander::new();
            expander.set_hide_expander(true);
            expander.set_indent_for_depth(false);
            expander.set_indent_for_icon(false);
            expander.set_hexpand(true);
            expander.add_css_class("sidebar-tree-row");
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

        // Monochrome Tabler outline icons, tinted to the theme's foreground color.
        let icon_color = state.borrow().cfg.theme.foreground.clone();
        let icons_dir = crate::icons::ensure(&icon_color);
        {
            let active_file = active_file.clone();
            let visible_rows = visible_rows.clone();
            let visible_disclosures = visible_disclosures.clone();
            let root_disclosure = root_disclosure.clone();
            let icons_dir = icons_dir.clone();
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
                let Some(branch) = indent.next_sibling().and_downcast::<Separator>() else {
                    return;
                };
                let Some(content) = row.last_child().and_downcast::<GtkBox>() else {
                    return;
                };
                let Some(disclosure) = content.first_child().and_downcast::<Image>() else {
                    return;
                };
                let Some(image) = disclosure.next_sibling().and_downcast::<Image>() else {
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
                    // One compact 16px level. The cell border forms a continuous
                    // vertical guide while the adjacent Separator forms its branch.
                    let cell = GtkBox::new(Orientation::Horizontal, 0);
                    cell.set_size_request(10, -1);
                    cell.set_margin_start(6);
                    cell.add_css_class("indent-guide");
                    indent.append(&cell);
                }
                branch.set_visible(treerow.depth() > 0);

                if let Some(info) = treerow.item().and_downcast::<gio::FileInfo>() {
                    label.set_label(&info.display_name());
                    let is_dir = info.file_type() == gio::FileType::Directory;
                    if is_dir {
                        disclosure.set_icon_name(Some(if treerow.is_expanded() {
                            "pan-down-symbolic"
                        } else {
                            "pan-end-symbolic"
                        }));
                    } else {
                        disclosure.clear();
                    }
                    set_css_class(&expander, "workspace-root", treerow.depth() == 0);
                    set_css_class(&expander, "directory-row", is_dir);
                    set_css_class(&expander, "file-row", !is_dir);
                    if treerow.depth() == 0 {
                        *root_disclosure.borrow_mut() = Some(disclosure.downgrade());
                    }

                    // Per-type icon from the bundled Tabler outline set, rasterized by
                    // librsvg at the exact device-pixel size (display scale, floored
                    // at 2× so it stays crisp on HiDPI even before the row knows its
                    // monitor) instead of letting GtkImage scale a natural-size bitmap.
                    let name = info.name();
                    let scale = image.scale_factor().max(2);
                    let path = file_of(&info).and_then(|file| file.path());
                    let is_active = path
                        .as_ref()
                        .is_some_and(|path| active_file.borrow().as_ref() == Some(path));
                    match crate::icons::icon_texture(
                        &icons_dir,
                        &name.to_string_lossy(),
                        is_dir,
                        14 * scale,
                    ) {
                        Some(tex) => image.set_paintable(Some(&tex)),
                        None => image.clear(),
                    }

                    if let Some(path) = path {
                        set_css_class(&expander, "active-file", is_active);
                        visible_rows
                            .borrow_mut()
                            .insert(path.clone(), expander.downgrade());
                        // Key the chevron by the stable path, not item.position(): a
                        // sibling expand/collapse shifts positions without re-binding,
                        // so a position key would go stale and flip the wrong chevron.
                        visible_disclosures
                            .borrow_mut()
                            .insert(path, disclosure.downgrade());
                    }
                }
            });
        }

        {
            let visible_rows = visible_rows.clone();
            let visible_disclosures = visible_disclosures.clone();
            factory.connect_unbind(move |_f, obj| {
                let Some(item) = obj.downcast_ref::<ListItem>() else {
                    return;
                };
                if let Some(treerow) = item.item().and_downcast::<TreeListRow>() {
                    if let Some(info) = treerow.item().and_downcast::<gio::FileInfo>() {
                        if let Some(path) = file_of(&info).and_then(|file| file.path()) {
                            visible_rows.borrow_mut().remove(&path);
                            visible_disclosures.borrow_mut().remove(&path);
                        }
                    }
                }
                if let Some(expander) = item.child().and_downcast::<TreeExpander>() {
                    expander.remove_css_class("active-file");
                    expander.remove_css_class("workspace-root");
                    expander.remove_css_class("directory-row");
                    expander.remove_css_class("file-row");
                }
            });
        }

        let list = ListView::new(Some(selection), Some(factory));
        list.set_single_click_activate(true);
        let st = state.clone();
        let active = active_file.clone();
        let rows = visible_rows.clone();
        let disclosures = visible_disclosures.clone();
        let normal_icons = icons_dir.clone();
        list.connect_activate(move |lv, pos| {
            let Some(model) = lv.model() else { return };
            let Some(row) = model.item(pos).and_downcast::<TreeListRow>() else {
                return;
            };
            let Some(info) = row.item().and_downcast::<gio::FileInfo>() else {
                return;
            };
            // A directory row expands only when it actually has a child model
            // (now true for symlinked dirs too). One that doesn't — e.g. a
            // symlinked dir left unfollowed under TCODE_AUTOEXPAND — falls
            // through and is opened instead of toggling a childless row.
            if info.file_type() == gio::FileType::Directory && row.is_expandable() {
                let expanded = !row.is_expanded();
                row.set_expanded(expanded);
                if let Some(disclosure) = file_of(&info)
                    .and_then(|f| f.path())
                    .and_then(|path| disclosures.borrow().get(&path).and_then(glib::WeakRef::upgrade))
                {
                    disclosure.set_icon_name(Some(if expanded {
                        "pan-down-symbolic"
                    } else {
                        "pan-end-symbolic"
                    }));
                }
            } else if let Some(path) = file_of(&info).and_then(|f| f.path()) {
                *active.borrow_mut() = Some(path.clone());
                rows.borrow_mut().retain(|row_path, weak| {
                    let Some(widget) = weak.upgrade() else {
                        return false;
                    };
                    let is_active = *row_path == path;
                    set_css_class(&widget, "active-file", is_active);
                    if let Some(image) = file_icon_for(&widget) {
                        let scale = image.scale_factor().max(2);
                        let name = row_path
                            .file_name()
                            .map(|name| name.to_string_lossy())
                            .unwrap_or_default();
                        if let Some(texture) = crate::icons::icon_texture(
                            &normal_icons,
                            &name,
                            row_path.is_dir(),
                            14 * scale,
                        ) {
                            image.set_paintable(Some(&texture));
                        }
                    }
                    true
                });
                open_file(&st, &path);
            } else {
                eprintln!("tcode: could not resolve path for sidebar item");
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
