//! GTK CSS + color helpers. Note: VTE *cell* colors are set via the VTE API
//! (see `pane.rs`); CSS here only styles the widget chrome (borders, padding).

use gtk4::gdk::{Display, RGBA};
use gtk4::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION};
use tessera_core::config::Theme;

/// Parse a `#rrggbb` string into an RGBA, falling back to opaque black.
pub fn rgba(hex: &str) -> RGBA {
    RGBA::parse(hex).unwrap_or_else(|_| RGBA::new(0.0, 0.0, 0.0, 1.0))
}

/// Install the application-wide stylesheet for the current display.
pub fn install_css(theme: &Theme, font: &str, font_size: u32) {
    let bg = &theme.background;
    let fg = &theme.foreground;
    let accent = &theme.accent;
    let surface = &theme.surface;
    let border = &theme.border;
    let css = format!(
        ".grid-root {{ background-color: {bg}; }}\n\
         .pane {{ background-color: {bg}; }}\n\
         .focus-ring {{ border: 1px solid transparent; }}\n\
         .pane.active-pane .focus-ring {{ border-color: #e0af68; }}\n\
         paned > separator {{ background-color: {border}; }}\n\
         .pick {{ font-size: 22px; font-weight: bold; border-radius: 12px; }}\n\
         .picker-root {{ background-color: {bg}; }}\n\
         .sidebar {{ background-color: {bg}; border-right: 1px solid {border}; \
                     font-family: \"Noto Sans\", sans-serif; font-size: 12px; }}\n\
         .sidebar-header {{ padding: 8px 10px; color: {fg}; font-weight: bold; }}\n\
         .sidebar label {{ color: {fg}; }}\n\
         .sidebar row {{ padding: 0 4px; min-height: 22px; border-radius: 4px; }}\n\
         .sidebar row:hover {{ background-color: transparent; }}\n\
         .sidebar .hovered {{ background-color: alpha({fg}, 0.08); border-radius: 4px; }}\n\
         .sidebar row:selected {{ background-color: alpha({accent}, 0.22); }}\n\
         .sidebar .indent-guide {{ border-left: 1px solid alpha({fg}, 0.32); }}\n\
         .tessera-titlebar {{ min-height: 24px; background-color: {bg}; \
                              box-shadow: none; border: none; color: {fg}; }}\n\
         .tessera-titlebar button {{ min-height: 0; min-width: 0; padding: 2px 6px; margin: 0; }}\n\
         .editor {{ background-color: {bg}; border-left: 1px solid {border}; }}\n\
         .editor header {{ min-height: 0; background-color: {surface}; }}\n\
         .editor header tab {{ min-height: 0; padding: 1px 8px; }}\n\
         .editor header tab button {{ min-height: 0; min-width: 0; padding: 2px; }}\n\
         .editor-view {{ font-family: \"{font}\", monospace; font-size: {font_size}pt; }}\n\
         .editor-view, .editor-view text {{ background-color: {bg}; color: {fg}; }}\n\
         .editor-view gutter {{ background-color: {bg}; }}\n\
         .image-view {{ background-color: {surface}; }}\n\
         .media-view {{ background-color: #000; }}\n\
         .viewer-toolbar {{ background-color: {bg}; padding: 0 6px; \
                            border-bottom: 1px solid {border}; }}\n\
         .viewer-toolbar button {{ min-height: 0; min-width: 0; padding: 1px 6px; \
                                   background: none; border: none; box-shadow: none; \
                                   color: alpha({fg}, 0.7); }}\n\
         .viewer-toolbar button:hover {{ background: alpha({fg}, 0.1); color: {fg}; }}\n\
         .viewer-zoom-btn {{ font-size: 15px; font-weight: 600; \
                             padding: 0 9px; min-width: 22px; }}\n\
         .viewer-zoom-pct {{ color: alpha({fg}, 0.7); min-width: 48px; \
                             padding: 0 4px; font-size: 12px; }}\n\
         .doc-view {{ background-color: {surface}; }}\n\
         .doc-page {{ background-color: white; box-shadow: 0 1px 8px rgba(0,0,0,0.55); }}\n\
         .csv-table {{ background-color: {bg}; }}\n\
         .csv-table > header {{ background-color: {surface}; }}\n\
         .csv-table > header button {{ font-weight: bold; color: {fg}; }}\n\
         .csv-cell {{ font-family: \"{font}\", monospace; font-size: {font_size}pt; \
                      padding: 2px 0; }}\n\
         .csv-note {{ color: alpha({fg}, 0.6); background-color: {surface}; \
                      padding: 4px 10px; font-size: 11px; }}\n\
         .csv-col-0 {{ color: #e06c75; }} .csv-col-1 {{ color: #98c379; }}\n\
         .csv-col-2 {{ color: #61afef; }} .csv-col-3 {{ color: #e5c07b; }}\n\
         .csv-col-4 {{ color: #c678dd; }} .csv-col-5 {{ color: #56b6c2; }}\n\
         .csv-col-6 {{ color: #d19a66; }} .csv-col-7 {{ color: #abb2bf; }}\n\
         .fallback-card {{ padding: 28px; }}\n\
         .fallback-title {{ font-size: 15px; font-weight: bold; color: {fg}; }}\n\
         .fallback-meta {{ color: alpha({fg}, 0.55); }}\n\
         .fallback-open {{ margin-top: 10px; }}\n\
         .bridgeshot-window {{ background-color: {bg}; }}\n\
         .bridgeshot-canvas {{ background-color: {bg}; }}\n\
         .bridgeshot-toolbar {{ background-color: {surface}; padding: 5px 8px; \
                                border-bottom: 1px solid {border}; }}\n\
         .bridgeshot-toolbar button {{ min-height: 0; padding: 3px 10px; }}\n\
         .bridgeshot-tool:checked {{ background-color: alpha({accent}, 0.30); \
                                     box-shadow: inset 0 0 0 1px {accent}; }}\n\
         .bridgeshot-swatches {{ margin-left: 4px; }}\n\
         .bridgeshot-swatch {{ min-width: 20px; min-height: 20px; padding: 0; \
                               border-radius: 10px; margin: 0 1px; }}\n\
         .bridgeshot-swatch.selected {{ box-shadow: 0 0 0 2px {fg}; }}\n\
         .swatch-0 {{ background-image: none; background-color: #e84d5b; }}\n\
         .swatch-1 {{ background-image: none; background-color: #7aa2f7; }}\n\
         .swatch-2 {{ background-image: none; background-color: #e0af68; }}\n\
         .swatch-3 {{ background-image: none; background-color: #8cc673; }}\n\
         .swatch-4 {{ background-image: none; background-color: #f2f2f7; }}\n\
         .swatch-5 {{ background-image: none; background-color: #1a1c26; }}\n\
         .bridgeshot-gallery {{ background-color: {surface}; }}\n\
         .bridgeshot-thumb {{ padding: 2px; border-radius: 4px; }}\n\
         .bridgeshot-thumb.selected {{ box-shadow: inset 0 0 0 2px {accent}; }}\n\
         .bridgeshot-text-entry {{ background-color: {bg}; color: {fg}; \
                                   box-shadow: inset 0 0 0 1px {accent}; }}\n\
         .bridgeshot-status {{ color: alpha({fg}, 0.7); padding: 6px 10px; }}"
    );
    let provider = CssProvider::new();
    provider.load_from_string(&css);
    if let Some(display) = Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
}
