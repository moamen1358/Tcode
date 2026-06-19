//! CSV / TSV table viewer — the VS Code "Rainbow CSV" experience: parse with the
//! `csv` crate (RFC 4180 quoting + escapes), then show the rows in a scrollable,
//! virtualized `GtkColumnView` with a header row, resizable columns, and
//! rainbow-colored columns for readability. Returns `None` for binary / non-UTF-8
//! input so the caller can fall back to the plain text viewer or info card.

use std::cell::Ref;
use std::path::Path;

use gtk4::glib::BoxedAnyObject;
use gtk4::pango::EllipsizeMode;
use gtk4::prelude::*;
use gtk4::{
    gio, Box as GtkBox, ColumnView, ColumnViewColumn, Label, ListItem, NoSelection, Orientation,
    PolicyType, ScrolledWindow, SignalListItemFactory, Widget,
};

/// Cap rows materialized into the model; ColumnView is virtualized so this only
/// guards pathological files (the note tells the user when it kicks in).
const MAX_ROWS: usize = 100_000;
/// Number of distinct column colors to cycle through.
const COLORS: usize = 8;

/// Build a table viewer for `path`, or `None` if it isn't decodable CSV-ish text.
pub fn csv_viewer(path: &Path) -> Option<Widget> {
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None; // binary → caller falls back
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.trim().is_empty() {
        return None;
    }

    let delim = detect_delimiter(&text, path);
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(false)
        .flexible(true)
        .from_reader(text.as_bytes());

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut truncated = false;
    for rec in rdr.records() {
        let Ok(rec) = rec else { continue };
        rows.push(rec.iter().map(|s| s.to_string()).collect());
        if rows.len() >= MAX_ROWS {
            truncated = true;
            break;
        }
    }
    if rows.is_empty() {
        return None;
    }

    // A real table needs >1 column or >1 row to be worth a grid; otherwise let
    // the caller show it as text (avoids a one-cell "table" for prose .txt).
    let ncols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    if ncols < 2 && rows.len() < 2 {
        return None;
    }

    let headers: Vec<String> = rows.first().cloned().unwrap_or_default();
    let data: Vec<Vec<String>> = if rows.len() > 1 {
        rows.split_off(1)
    } else {
        Vec::new()
    };

    let store = gio::ListStore::new::<BoxedAnyObject>();
    for row in data {
        store.append(&BoxedAnyObject::new(row));
    }
    let selection = NoSelection::new(Some(store));
    let column_view = ColumnView::new(Some(selection));
    column_view.add_css_class("csv-table");
    column_view.set_show_row_separators(true);
    column_view.set_show_column_separators(true);

    for c in 0..ncols {
        let title = headers
            .get(c)
            .map(|h| h.trim())
            .filter(|h| !h.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("#{}", c + 1));

        let color_class = format!("csv-col-{}", c % COLORS);
        let factory = SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let Some(item) = item.downcast_ref::<ListItem>() else {
                return;
            };
            let label = Label::new(None);
            label.set_xalign(0.0);
            label.set_ellipsize(EllipsizeMode::End);
            label.set_margin_start(8);
            label.set_margin_end(8);
            label.add_css_class("csv-cell");
            label.add_css_class(&color_class);
            item.set_child(Some(&label));
        });
        factory.connect_bind(move |_, item| {
            let Some(item) = item.downcast_ref::<ListItem>() else {
                return;
            };
            let Some(obj) = item.item() else { return };
            let Ok(boxed) = obj.downcast::<BoxedAnyObject>() else {
                return;
            };
            let row: Ref<Vec<String>> = boxed.borrow();
            let text = row.get(c).map(String::as_str).unwrap_or("");
            if let Some(label) = item.child().and_downcast::<Label>() {
                label.set_text(text);
            }
        });

        let column = ColumnViewColumn::new(Some(&title), Some(factory));
        column.set_resizable(true);
        // Share the panel width across columns (instead of collapsing to the
        // ellipsized label's tiny natural width); the user can still drag-resize.
        column.set_expand(true);
        column_view.append_column(&column);
    }

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&column_view)
        .build();

    if !truncated {
        return Some(scroller.upcast());
    }
    let root = GtkBox::new(Orientation::Vertical, 0);
    let note = Label::new(Some(&format!(
        "Large file — showing the first {MAX_ROWS} rows."
    )));
    note.add_css_class("csv-note");
    note.set_xalign(0.0);
    root.append(&note);
    root.append(&scroller);
    Some(root.upcast())
}

/// Pick the delimiter: `.tsv` is always tab; otherwise the most frequent of
/// `, ; \t |` on the first non-empty line.
fn detect_delimiter(text: &str, path: &Path) -> u8 {
    let is_tsv = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("tsv"));
    if is_tsv {
        return b'\t';
    }
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    [b',', b';', b'\t', b'|']
        .into_iter()
        .map(|d| (d, line.bytes().filter(|&b| b == d).count()))
        .filter(|&(_, n)| n > 0)
        .max_by_key(|&(_, n)| n)
        .map(|(d, _)| d)
        .unwrap_or(b',')
}
