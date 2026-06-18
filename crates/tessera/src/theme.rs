//! GTK CSS + color helpers. Note: VTE *cell* colors are set via the VTE API
//! (see `pane.rs`); CSS here only styles the widget chrome (borders, padding).

use gtk4::gdk::{Display, RGBA};
use gtk4::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION};

/// Parse a `#rrggbb` string into an RGBA, falling back to opaque black.
pub fn rgba(hex: &str) -> RGBA {
    RGBA::parse(hex).unwrap_or_else(|_| RGBA::new(0.0, 0.0, 0.0, 1.0))
}

/// Install the application-wide stylesheet for the current display.
pub fn install_css(accent: &str, bg: &str) {
    let css = format!(
        ".grid-root {{ background-color: {bg}; }}\n\
         .pane {{ border-radius: 8px; border: 2px solid transparent; background-color: {bg}; }}\n\
         .pane.active-pane {{ border: 2px solid {accent}; }}\n\
         .exited {{ color: #f38ba8; background-color: rgba(30,30,46,0.88); \
                    padding: 6px 12px; border-radius: 6px; }}\n\
         .pick {{ font-size: 22px; font-weight: bold; border-radius: 12px; }}\n\
         .picker-root {{ background-color: {bg}; }}\n\
         .sidebar {{ background-color: #181825; border-right: 1px solid #313244; }}\n\
         .sidebar-header {{ padding: 8px 10px; color: #a6adc8; font-weight: bold; }}\n\
         .row-dir {{ color: {accent}; padding: 3px 10px; }}\n\
         .row-file {{ color: #cdd6f4; padding: 3px 10px; }}\n\
         .tessera-titlebar {{ min-height: 28px; background-color: #181825; \
                              box-shadow: none; border: none; color: #cdd6f4; }}"
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
