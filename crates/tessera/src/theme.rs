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
         .pane.active-pane {{ box-shadow: inset 0 0 0 1px {accent}; }}\n\
         paned > separator {{ background-color: {border}; }}\n\
         .pick {{ font-size: 22px; font-weight: bold; border-radius: 12px; }}\n\
         .picker-root {{ background-color: {bg}; }}\n\
         .sidebar {{ background-color: {bg}; border-right: 1px solid {border}; }}\n\
         .sidebar-header {{ padding: 8px 10px; color: {fg}; font-weight: bold; }}\n\
         .sidebar label {{ color: {fg}; }}\n\
         .sidebar row {{ padding: 0 4px; min-height: 24px; border-radius: 4px; }}\n\
         .sidebar row:hover {{ background-color: alpha({fg}, 0.06); }}\n\
         .sidebar row:selected {{ background-color: alpha({accent}, 0.22); }}\n\
         .sidebar .indent-guide {{ border-left: 1px solid alpha({fg}, 0.32); }}\n\
         .sidebar .open {{ background-color: alpha(#000000, 0.60); border-radius: 4px; }}\n\
         .tessera-titlebar {{ min-height: 28px; background-color: {surface}; \
                              box-shadow: none; border: none; color: {fg}; }}\n\
         .editor {{ background-color: {bg}; border-left: 1px solid {border}; }}\n\
         .editor header {{ min-height: 0; background-color: {surface}; }}\n\
         .editor header tab {{ min-height: 0; padding: 1px 8px; }}\n\
         .editor header tab button {{ min-height: 0; min-width: 0; padding: 2px; }}\n\
         .editor-view {{ font-family: \"{font}\", monospace; font-size: {font_size}pt; }}\n\
         .editor-view, .editor-view text {{ background-color: {bg}; color: {fg}; }}"
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
