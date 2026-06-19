//! CSV / TSV "rainbow" viewer — VS Code Rainbow-CSV style: the raw text with line
//! numbers and no wrapping (scroll left/right to read a whole wide row), where
//! each delimiter-separated field is tinted by its column index so columns stay
//! visually distinct. Returns `None` for binary / non-UTF-8 input so the caller
//! can fall back to the plain text viewer or info card.
//!
//! Coloring is applied lazily to the lines currently in view (re-applied on
//! scroll) so even a 17k-row × 41-column file opens instantly instead of
//! tagging hundreds of thousands of fields up front.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{PolicyType, ScrolledWindow, TextTag, Widget, WrapMode};
use sourceview5::prelude::*;
use sourceview5::{Buffer, View};

/// Don't load multi-GB files into the editor synchronously.
const MAX_BYTES: u64 = 64 * 1024 * 1024;

/// Per-column foreground colors (cycled). Readable on the dark theme.
const COLORS: [&str; 8] = [
    "#e06c75", "#98c379", "#61afef", "#e5c07b", "#c678dd", "#56b6c2", "#d19a66", "#d7dae0",
];

/// Build a rainbow CSV/TSV viewer for `path`, or `None` if it isn't decodable text.
pub fn csv_viewer(path: &Path) -> Option<Widget> {
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) > MAX_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None; // binary → caller falls back
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.trim().is_empty() {
        return None;
    }
    let delim = detect_delimiter(&text, path) as char;

    let buffer = Buffer::new(None);
    if let Some(scheme) = sourceview5::StyleSchemeManager::default().scheme("Adwaita-dark") {
        buffer.set_style_scheme(Some(&scheme));
    }
    buffer.set_text(&text);

    // One color tag per column slot, cycled.
    let tags: Vec<TextTag> = COLORS
        .iter()
        .map(|c| {
            let tag = TextTag::builder().foreground(*c).build();
            buffer.tag_table().add(&tag);
            tag
        })
        .collect();

    let view = View::with_buffer(&buffer);
    view.set_show_line_numbers(true);
    view.set_monospace(true);
    view.set_editable(false);
    view.set_cursor_visible(false);
    view.set_wrap_mode(WrapMode::None); // no wrap → horizontal scroll for wide rows
    view.set_left_margin(8);
    view.add_css_class("editor-view");

    let scroller = ScrolledWindow::builder()
        .hscrollbar_policy(PolicyType::Automatic)
        .vscrollbar_policy(PolicyType::Automatic)
        .vexpand(true)
        .hexpand(true)
        .child(&view)
        .build();

    // Line text kept around so we can compute field boundaries on demand.
    let lines: Rc<Vec<String>> = Rc::new(text.lines().map(str::to_string).collect());
    let tagged: Rc<RefCell<HashSet<i32>>> = Rc::new(RefCell::new(HashSet::new()));

    // Color the lines currently visible (plus a small margin), once each.
    let recolor: Rc<dyn Fn()> = {
        let (view, buffer, lines, tags, tagged) =
            (view.clone(), buffer.clone(), lines.clone(), tags.clone(), tagged.clone());
        Rc::new(move || {
            let rect = view.visible_rect();
            if rect.height() == 0 {
                return;
            }
            let top = view.line_at_y(rect.y()).0.line();
            let bot = view.line_at_y(rect.y() + rect.height()).0.line();
            let last = lines.len() as i32 - 1;
            let from = (top - 4).max(0);
            let to = (bot + 4).min(last);
            let mut done = tagged.borrow_mut();
            for line in from..=to {
                if !done.insert(line) {
                    continue;
                }
                let Some(content) = lines.get(line as usize) else {
                    continue;
                };
                for (col, (s, e)) in field_ranges(content, delim).into_iter().enumerate() {
                    if e <= s {
                        continue;
                    }
                    if let (Some(a), Some(b)) =
                        (buffer.iter_at_line_offset(line, s), buffer.iter_at_line_offset(line, e))
                    {
                        buffer.apply_tag(&tags[col % tags.len()], &a, &b);
                    }
                }
            }
        })
    };

    // Recolor on scroll and when the view first gets a size (adjustment changes).
    let vadj = scroller.vadjustment();
    {
        let recolor = recolor.clone();
        vadj.connect_value_changed(move |_| recolor());
    }
    {
        let recolor = recolor.clone();
        vadj.connect_changed(move |_| recolor());
    }
    // Initial pass once laid out.
    gtk4::glib::idle_add_local_once(move || recolor());

    Some(scroller.upcast())
}

/// Character-offset `(start, end)` ranges of each field in `line` (quote-aware,
/// so a delimiter inside `"…"` doesn't split a field).
fn field_ranges(line: &str, delim: char) -> Vec<(i32, i32)> {
    let mut ranges = Vec::new();
    let mut start = 0i32;
    let mut idx = 0i32;
    let mut in_quotes = false;
    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == delim && !in_quotes {
            ranges.push((start, idx));
            start = idx + 1;
        }
        idx += 1;
    }
    ranges.push((start, idx));
    ranges
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
