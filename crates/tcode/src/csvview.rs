//! CSV / TSV "rainbow" viewer — VS Code Rainbow-CSV style: the raw text with line
//! numbers and no wrapping (scroll left/right to read a whole wide row), where
//! each delimiter-separated field is tinted by its column index so columns stay
//! visually distinct. Returns `None` for binary / non-UTF-8 input so the caller
//! can fall back to the plain text viewer or info card.
//!
//! Coloring is applied lazily to the lines currently in view (re-applied on
//! scroll) so even a 17k-row × 41-column file opens instantly instead of
//! tagging hundreds of thousands of fields up front.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{PolicyType, ScrolledWindow, TextTag, Widget, WrapMode};
use sourceview5::prelude::*;
use sourceview5::{Buffer, View};

/// Per-column foreground colors (cycled). Readable on the dark theme.
const COLORS: [&str; 8] = [
    "#e06c75", "#98c379", "#61afef", "#e5c07b", "#c678dd", "#56b6c2", "#d19a66", "#d7dae0",
];

/// Build a rainbow CSV/TSV viewer for `path`, or `None` if it isn't decodable text.
pub fn csv_viewer(path: &Path) -> Option<Widget> {
    if crate::loader::too_large(path) || crate::loader::looks_binary(path) {
        return None; // too large or binary → caller falls back to the text viewer
    }

    let buffer = Buffer::new(None);
    if let Some(scheme) = sourceview5::StyleSchemeManager::default().scheme("Adwaita-dark") {
        buffer.set_style_scheme(Some(&scheme));
    }

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

    // Track which lines have already been colored so each is tagged once.
    let tagged: Rc<RefCell<HashSet<i32>>> = Rc::new(RefCell::new(HashSet::new()));
    // Delimiter is detected once the file has loaded (off-thread); recolor reads
    // it from this cell so visible-line coloring uses the right delimiter.
    let delim: Rc<Cell<char>> = Rc::new(Cell::new(','));

    // Color the lines currently visible (plus a small margin), once each. Line
    // text is read straight from the buffer so field offsets match the exact line
    // model GTK splits on (handles lone-CR / CRLF) without a second full copy.
    let recolor: Rc<dyn Fn()> = {
        let (view, buffer, tags, tagged, delim) = (
            view.clone(),
            buffer.clone(),
            tags.clone(),
            tagged.clone(),
            delim.clone(),
        );
        Rc::new(move || {
            let rect = view.visible_rect();
            if rect.height() == 0 {
                return;
            }
            let top = view.line_at_y(rect.y()).0.line();
            let bot = view.line_at_y(rect.y() + rect.height()).0.line();
            let last = buffer.line_count() - 1;
            let from = (top - 4).max(0);
            let to = (bot + 4).min(last);
            let d = delim.get();
            let mut done = tagged.borrow_mut();
            let mut ranges: Vec<(i32, i32)> = Vec::new(); // reused across lines
            for line in from..=to {
                if !done.insert(line) {
                    continue;
                }
                let Some(start) = buffer.iter_at_line(line) else {
                    continue;
                };
                let mut end = start;
                end.forward_to_line_end(); // stops before the line terminator
                // Tag straight off the buffer text (a GString) with no extra String
                // copy, into a scratch Vec reused for every visible line.
                let content = buffer.text(&start, &end, false);
                field_ranges(content.as_str(), d, &mut ranges);
                for (col, &(s, e)) in ranges.iter().enumerate() {
                    if e <= s {
                        continue;
                    }
                    if let (Some(a), Some(b)) = (
                        buffer.iter_at_line_offset(line, s),
                        buffer.iter_at_line_offset(line, e),
                    ) {
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
    // Read the file off the main thread; once decoded, detect the delimiter, fill
    // the buffer, and color the first screen — so a big CSV doesn't freeze the UI
    // on open. The view appears immediately and populates when the read finishes.
    //
    // Coloring holds the whole document in a GtkTextBuffer, so cap the table preview
    // well under loader::MAX_TEXT_BYTES (64 MB): a tens-of-MB CSV would stall the main
    // thread on set_text and balloon RAM. Larger files get a short notice instead.
    const MAX_CSV_PREVIEW_BYTES: usize = 8 * 1024 * 1024;
    {
        let (buffer, delim, recolor) = (buffer.clone(), delim.clone(), recolor.clone());
        let p = path.to_path_buf();
        crate::loader::load_text_async(path, move |text| {
            if let Some(text) = text {
                if text.len() > MAX_CSV_PREVIEW_BYTES {
                    buffer.set_text(&format!(
                        "CSV too large to preview as a table ({} MB). Open it in a \
                         terminal pane (e.g. `less` or `column -s, -t`) instead.",
                        text.len() / (1024 * 1024)
                    ));
                    return;
                }
                delim.set(detect_delimiter(&text, &p) as char);
                buffer.set_text(&text);
                recolor();
            }
        });
    }

    Some(scroller.upcast())
}

/// Fill `out` with the character-offset `(start, end)` range of each field in
/// `line` (quote-aware, so a delimiter inside `"…"` doesn't split a field).
fn field_ranges(line: &str, delim: char, out: &mut Vec<(i32, i32)>) {
    out.clear();
    let mut start = 0i32;
    let mut idx = 0i32;
    let mut in_quotes = false;
    for ch in line.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if ch == delim && !in_quotes {
            out.push((start, idx));
            start = idx + 1;
        }
        idx += 1;
    }
    out.push((start, idx));
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
    // Count each candidate only outside quoted fields, so a quoted header cell
    // containing commas can't outvote the real delimiter.
    let count_outside_quotes = |delim: u8| {
        let mut n = 0usize;
        let mut in_quotes = false;
        for &b in line.as_bytes() {
            if b == b'"' {
                in_quotes = !in_quotes;
            } else if b == delim && !in_quotes {
                n += 1;
            }
        }
        n
    };
    [b',', b';', b'\t', b'|']
        .into_iter()
        .map(|d| (d, count_outside_quotes(d)))
        .filter(|&(_, n)| n > 0)
        .max_by_key(|&(_, n)| n)
        .map(|(d, _)| d)
        .unwrap_or(b',')
}
