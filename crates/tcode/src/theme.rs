//! GTK CSS + color helpers. Note: VTE *cell* colors are set via the VTE API
//! (see `pane.rs`); CSS here only styles the widget chrome (borders, padding).

use gtk4::gdk::{Display, RGBA};
use gtk4::{CssProvider, STYLE_PROVIDER_PRIORITY_APPLICATION};
use tcode_core::config::Theme;

/// Parse a `#rrggbb` string into an RGBA, falling back to opaque black.
pub fn rgba(hex: &str) -> RGBA {
    RGBA::parse(hex).unwrap_or_else(|_| RGBA::new(0.0, 0.0, 0.0, 1.0))
}

/// Normalize a config color into a CSS-safe `rgba()` literal (opaque black on
/// anything unparseable). Config strings are interpolated into the global
/// stylesheet, so a malformed/hostile value must not break parsing or inject CSS.
fn css_color(s: &str) -> String {
    let c = rgba(s);
    format!(
        "rgba({},{},{},{})",
        (c.red() * 255.0).round() as u8,
        (c.green() * 255.0).round() as u8,
        (c.blue() * 255.0).round() as u8,
        c.alpha()
    )
}

/// Darken a `#rrggbb` color, keeping `factor` (0..1) of each RGB channel, and
/// return a CSS-safe `rgba()` literal. Used to derive the grid backdrop from the
/// theme background so the floating terminal cards sit on a darker canvas that
/// tracks whatever theme is configured (rather than a hard-coded near-black).
fn darken_css(s: &str, factor: f32) -> String {
    let c = rgba(s);
    let f = factor.clamp(0.0, 1.0);
    format!(
        "rgba({},{},{},{})",
        (c.red() * f * 255.0).round() as u8,
        (c.green() * f * 255.0).round() as u8,
        (c.blue() * f * 255.0).round() as u8,
        c.alpha()
    )
}

/// Fraction of each background RGB channel kept for the deep "blanket" backdrop
/// behind the floating cards (`.work-area`, `.grid-root`, the Paned separators) —
/// and, so it matches, the image/document viewer.
const BACKDROP_FACTOR: f32 = 0.45;

/// The deep backdrop as an RGBA, for code that paints it directly (the image
/// viewer's Cairo draw). Mirrors the CSS `darken_css(background, BACKDROP_FACTOR)`
/// so the viewer reads as the same black as the canvas behind the cards.
pub fn viewer_backdrop(theme: &Theme) -> RGBA {
    let c = rgba(&theme.background);
    RGBA::new(
        c.red() * BACKDROP_FACTOR,
        c.green() * BACKDROP_FACTOR,
        c.blue() * BACKDROP_FACTOR,
        c.alpha(),
    )
}

/// Keep only characters legal in a CSS font-family name, so a config font can't
/// break out of the quoted family or inject declarations.
fn css_font(s: &str) -> String {
    let f: String = s
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_'))
        .collect();
    if f.trim().is_empty() {
        "monospace".to_string()
    } else {
        f
    }
}

/// Multiply every `font-size: Npx|pt` in the stylesheet by `scale`, so the whole
/// UI's text grows/shrinks together (px font-sizes aren't affected by font DPI,
/// so we scale them ourselves). Units are preserved.
fn scale_css_fonts(css: &str, scale: f64) -> String {
    if (scale - 1.0).abs() < f64::EPSILON {
        return css.to_string();
    }
    let mut out = String::with_capacity(css.len() + 64);
    let mut rest = css;
    while let Some(i) = rest.find("font-size:") {
        let after = i + "font-size:".len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        let nonspace = rest.find(|c: char| c != ' ').unwrap_or(rest.len());
        out.push_str(&rest[..nonspace]);
        rest = &rest[nonspace..];
        let num_end = rest
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(rest.len());
        match rest[..num_end].parse::<f64>() {
            Ok(n) if num_end > 0 => {
                out.push_str(&((n * scale).round().max(1.0) as i64).to_string());
                rest = &rest[num_end..];
            }
            _ => {}
        }
    }
    out.push_str(rest);
    out
}

/// Install the application-wide stylesheet for the current display. `scale` is the
/// whole-UI zoom (1.0 = 100%).
pub fn install_css(theme: &Theme, font: &str, font_size: u32, scale: f64) {
    let bg = css_color(&theme.background);
    let fg = css_color(&theme.foreground);
    let accent = css_color(&theme.accent);
    let surface = css_color(&theme.surface);
    let border = css_color(&theme.border);
    // Backdrop behind the floating terminal cards: a darker shade of the theme
    // background, so the panes read as lighter cards hovering over a canvas.
    let backdrop = darken_css(&theme.background, BACKDROP_FACTOR);
    let font = css_font(font);
    let sidebar_font_size = (f64::from(font_size) * 0.8).max(7.0);
    let css = format!(
        ".grid-root {{ background-color: {backdrop}; padding: 4.5px; }}\n\
         .pane {{ background-color: {bg}; margin: 4.5px; \
                  box-shadow: 0 2px 8px rgba(0,0,0,0.55); }}\n\
         .focus-ring {{ border: 1px solid transparent; }}\n\
         .pane.active-pane .focus-ring {{ border-color: #e0af68; }}\n\
         .overlay-scrim {{ background-color: rgba(0,0,0,0.46); }}\n\
         .clip-palette {{ background-color: {surface}; border-radius: 0; padding: 0; \
                          box-shadow: 0 18px 54px rgba(0,0,0,0.58); \
                          border: 1px solid alpha({fg}, 0.16); \
                          font-family: \"{font}\", monospace; }}\n\
         .clip-pal-list {{ background-color: transparent; padding: 0; }}\n\
         .clip-pal-row {{ min-height: 44px; background-color: transparent; \
                          border-bottom: 1px solid alpha({fg}, 0.10); }}\n\
         .clip-pal-row.active {{ background-color: alpha({fg}, 0.035); }}\n\
         .clip-pal-copy {{ min-height: 44px; padding: 0 12px; margin: 0; \
                          background: none; border: none; border-radius: 0; box-shadow: none; }}\n\
         .clip-pal-copy:hover {{ background-color: alpha({fg}, 0.045); }}\n\
         .clip-pal-copy:active {{ background-color: alpha({fg}, 0.07); }}\n\
         .clip-pal-text {{ color: alpha({fg}, 0.92); font-size: 13px; \
                          font-family: \"{font}\", monospace; }}\n\
         .clip-pal-status {{ min-width: 76px; }}\n\
         .clip-pal-status label {{ color: alpha({fg}, 0.50); font-size: 11px; \
                          font-family: \"{font}\", monospace; }}\n\
         .clip-pal-status image {{ color: #bb9af7; }}\n\
         .clip-pal-status.copied label {{ color: #bb9af7; }}\n\
         .clip-pal-pin, .clip-pal-delete {{ min-width: 36px; min-height: 44px; \
                          padding: 0; margin: 0; background: none; border: none; \
                          border-radius: 0; box-shadow: none; color: alpha({fg}, 0.58); }}\n\
         .clip-pal-pin:hover {{ background-color: alpha({fg}, 0.05); color: {fg}; }}\n\
         .clip-pal-row.pinned .clip-pal-pin {{ color: #ff9e64; }}\n\
         .clip-pal-delete:hover {{ background-color: alpha(#f7768e, 0.10); color: #f7768e; }}\n\
         .clip-empty {{ color: alpha({fg}, 0.38); font-size: 12px; padding: 14px 12px; }}\n\
         .shot-preview {{ background-color: {surface}; border-radius: 10px; padding: 5px; \
                          box-shadow: 0 14px 36px rgba(0,0,0,0.66); \
                          border: 1px solid alpha({accent}, 0.40); }}\n\
         .shot-preview .shot-preview-img {{ border-radius: 6px; }}\n\
         .shot-tray {{ background-color: {backdrop}; margin: 0; padding: 0; }}\n\
         .shot-tray .frame-gallery {{ background-color: {backdrop}; padding: 4.5px 4.5px 4.5px 0; }}\n\
         .shot-tray .frame-thumb {{ border-radius: 0; padding: 0; margin: 4.5px 4.5px 4.5px 0; }}\n\
         .shot-tray .frame-thumb:hover {{ box-shadow: inset 0 0 0 2px alpha({accent}, 0.75); }}\n\
         .shot-tray .frame-thumb.selected {{ box-shadow: inset 0 0 0 2px {accent}; }}\n\
         .work-area {{ background-color: {backdrop}; }}\n\
         paned > separator {{ background-color: {backdrop}; }}\n\
         .picker-root {{ background-color: {bg}; }}\n\
         .sidebar {{ background-color: {backdrop}; margin: 9px 0 9px 9px; \
                     box-shadow: 0 2px 8px rgba(0,0,0,0.55); \
                     font-family: \"Martian Mono\", monospace; font-size: {sidebar_font_size:.1}pt; }}\n\
         .sidebar label {{ color: alpha({fg}, 0.82); }}\n\
         .sidebar listview {{ background-color: transparent; color: {fg}; }}\n\
         .sidebar row {{ padding: 0; min-height: 25px; border-radius: 0; \
                         background-color: transparent; }}\n\
         .sidebar row:hover, .sidebar row:selected {{ background-color: transparent; }}\n\
         .sidebar-tree-header {{ min-height: 36px; padding: 0 10px; \
                                 border-bottom: 1px solid alpha({fg}, 0.06); }}\n\
         .sidebar-tree-header label {{ color: alpha({fg}, 0.68); font-size: 8pt; \
                                       font-weight: 600; letter-spacing: 1.2px; }}\n\
         .sidebar-tree-header .header-slash {{ color: alpha({fg}, 0.34); }}\n\
         .sidebar .sidebar-header-toggle {{ min-width: 20px; min-height: 20px; padding: 0; \
                                            background: none; border: none; box-shadow: none; \
                                            color: alpha({fg}, 0.62); }}\n\
         .sidebar .sidebar-header-toggle:hover {{ background-color: alpha({fg}, 0.06); \
                                                  color: {fg}; }}\n\
         .sidebar .sidebar-tree-row {{ min-height: 25px; margin: 0 4px; padding: 0 4px; \
                                       border: 1px solid transparent; border-radius: 2px; }}\n\
         .sidebar .sidebar-tree-row.hovered {{ background-color: alpha({fg}, 0.055); }}\n\
         .sidebar .workspace-root label {{ color: alpha({fg}, 0.96); font-weight: 600; }}\n\
         .sidebar .tree-disclosure {{ color: alpha({fg}, 0.68); }}\n\
         .sidebar .indent-guide {{ border-left: 1px solid alpha(#ff9e64, 0.78); }}\n\
         .sidebar .tree-branch {{ min-height: 1px; background-color: alpha(#ff9e64, 0.78); }}\n\
         .sidebar .file-row .indent-guide {{ border-left-color: alpha({fg}, 0.32); }}\n\
         .sidebar .file-row .tree-branch {{ background-color: alpha({fg}, 0.32); }}\n\
         .tcode-titlebar {{ min-height: 18px; background-color: {bg}; \
                              box-shadow: none; border: none; color: {fg}; }}\n\
         .tcode-titlebar button {{ min-height: 0; min-width: 0; padding: 1px 6px; margin: 0; }}\n\
         .tcode-titlebar .titlebar-toggle {{ background: none; box-shadow: none; border: none; \
                              color: alpha({fg}, 0.4); }}\n\
         .tcode-titlebar .titlebar-toggle:hover {{ background-color: alpha({fg}, 0.06); color: {fg}; }}\n\
         .tcode-titlebar .titlebar-toggle:checked {{ background: none; color: {fg}; }}\n\
         .editor {{ background-color: {bg}; margin: 9px 9px 9px 0; \
                    box-shadow: 0 2px 8px rgba(0,0,0,0.55); }}\n\
         .editor header {{ min-height: 0; background-color: {surface}; }}\n\
         .editor header tab {{ min-height: 0; padding: 1px 8px; }}\n\
         .editor header tab button {{ min-height: 0; min-width: 0; padding: 2px; }}\n\
         .editor-view {{ font-family: \"{font}\", monospace; font-size: {font_size}pt; }}\n\
         .editor-view, .editor-view text {{ background-color: {bg}; color: {fg}; }}\n\
         .editor-view gutter {{ background-color: {bg}; }}\n\
         .image-view {{ background-color: {backdrop}; }}\n\
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
         .doc-view {{ background-color: {backdrop}; }}\n\
         .doc-page {{ background-color: white; box-shadow: 0 1px 8px rgba(0,0,0,0.55); }}\n\
         .doc-status {{ color: alpha({fg}, 0.7); padding: 6px 10px; }}\n\
         .doc-error {{ color: #e84d5b; }}\n\
         .fallback-card {{ padding: 28px; }}\n\
         .fallback-title {{ font-size: 15px; font-weight: bold; color: {fg}; }}\n\
         .fallback-meta {{ color: alpha({fg}, 0.55); }}\n\
         .fallback-open {{ margin-top: 10px; }}\n\
         .frame-window {{ background-color: {bg}; }}\n\
         .frame-canvas {{ background-color: {bg}; }}\n\
         .frame-toolbar {{ background-color: alpha({bg}, 0.96); padding: 4px 10px; \
                                border-bottom: 1px solid {border}; }}\n\
         .frame-toolbar-center {{ background-color: {surface}; padding: 3px; \
                                       border: 1px solid alpha({fg}, 0.10); \
                                       border-radius: 7px; \
                                       box-shadow: 0 4px 14px rgba(0,0,0,0.22); }}\n\
         .frame-toolbar button {{ min-height: 0; min-width: 0; padding: 2px 6px; \
                                       border-radius: 5px; }}\n\
         .frame-tool-group {{ background-color: alpha({fg}, 0.05); padding: 1px; \
                                   border-radius: 6px; }}\n\
         .frame-tool {{ min-width: 38px; min-height: 26px; background: transparent; border: none; \
                             box-shadow: none; color: alpha({fg}, 0.76); }}\n\
         .frame-tool:hover {{ background-color: alpha({fg}, 0.08); color: {fg}; }}\n\
         .frame-tool:checked {{ background-color: alpha({accent}, 0.30); color: {fg}; \
                                     box-shadow: inset 0 0 0 1px alpha({accent}, 0.78); }}\n\
         .frame-utility {{ min-width: 28px; min-height: 26px; background: transparent; border: none; box-shadow: none; \
                                color: alpha({fg}, 0.72); }}\n\
         .frame-utility:hover {{ background-color: alpha({fg}, 0.08); color: {fg}; }}\n\
         .frame-actions button {{ min-width: 30px; min-height: 28px; padding: 2px 7px; }}\n\
         .frame-cancel {{ background: transparent; border: none; box-shadow: none; \
                                color: alpha({fg}, 0.72); }}\n\
         .frame-cancel:hover {{ background-color: alpha(#e84d5b, 0.16); color: #e84d5b; }}\n\
         .frame-save {{ font-weight: 600; border-radius: 5px; }}\n\
         .frame-swatches {{ margin-left: 2px; margin-right: 2px; }}\n\
         .frame-swatch {{ min-width: 18px; min-height: 18px; padding: 0; \
                               border-radius: 9px; margin: 0 1px; \
                               box-shadow: inset 0 0 0 1px rgba(255,255,255,0.18); }}\n\
         .frame-swatch:hover {{ box-shadow: inset 0 0 0 1px rgba(255,255,255,0.32), \
                                     0 0 0 2px alpha({fg}, 0.10); }}\n\
         .frame-swatch.selected {{ box-shadow: inset 0 0 0 1px rgba(255,255,255,0.38), \
                                        0 0 0 2px {fg}; }}\n\
         .swatch-0 {{ background-image: none; background-color: #e84d5b; }}\n\
         .swatch-1 {{ background-image: none; background-color: #7aa2f7; }}\n\
         .swatch-2 {{ background-image: none; background-color: #e0af68; }}\n\
         .swatch-3 {{ background-image: none; background-color: #8cc673; }}\n\
         .swatch-4 {{ background-image: none; background-color: #f2f2f7; }}\n\
         .swatch-5 {{ background-image: none; background-color: #1a1c26; }}\n\
         .frame-gallery {{ background-color: {surface}; }}\n\
         .frame-thumb {{ padding: 2px; border-radius: 4px; }}\n\
         .frame-thumb.selected {{ box-shadow: inset 0 0 0 2px {accent}; }}\n\
         .frame-text-entry {{ background-color: {bg}; color: {fg}; \
                                   box-shadow: inset 0 0 0 1px {accent}; }}\n\
         .clip-header {{ padding: 5px 8px 3px 10px; }}\n\
         .clip-header label {{ color: alpha({fg}, 0.45); font-size: 10px; \
                               font-weight: bold; letter-spacing: 1px; }}\n\
         .clip-clear {{ min-height: 0; min-width: 0; padding: 2px 5px; background: none; \
                        border: none; box-shadow: none; border-radius: 0; \
                        color: alpha({fg}, 0.4); }}\n\
         .clip-clear:hover {{ color: alpha({fg}, 0.8); background-color: alpha({fg}, 0.08); }}\n\
         .clip-card {{ background-color: {surface}; border-radius: 0; }}\n\
         .clip-card.pinned {{ background-color: alpha(#ff9e64, 0.12); }}\n\
         .clip-copy {{ padding: 6px 9px; background: none; border: none; \
                       box-shadow: none; border-radius: 0; }}\n\
         .clip-copy:hover {{ background-color: alpha({fg}, 0.05); }}\n\
         .clip-text {{ color: alpha({fg}, 0.85); font-size: 13px; \
                       font-family: \"{font}\", monospace; }}\n\
         .clip-pin, .clip-del {{ min-height: 0; min-width: 0; padding: 2px; \
                                 margin: 4px 2px 0 0; background: none; border: none; \
                                 box-shadow: none; border-radius: 0; color: alpha({fg}, 0.25); }}\n\
         .clip-pin:hover, .clip-del:hover {{ color: alpha({fg}, 0.75); \
                                             background-color: alpha({fg}, 0.08); }}\n\
         .clip-card.pinned .clip-text, .clip-card.pinned .clip-pin {{ color: #ffb38a; }}\n\
         .session-title {{ font-size: 32px; font-weight: bold; color: {fg}; }}\n\
         .session-subtitle {{ color: alpha({fg}, 0.45); font-size: 13px; }}\n\
         .session-card {{ background-color: {surface}; background-image: none; \
                          border: 1px solid {border}; \
                          border-radius: 0; padding: 13px 15px; }}\n\
         .session-card:hover {{ background-color: alpha({fg}, 0.05); border-color: #ff9e64; }}\n\
         .session-card-icon {{ color: #ff9e64; }}\n\
         .session-delete {{ background: transparent; background-image: none; border: none; \
                            box-shadow: none; min-height: 0; min-width: 0; padding: 2px; \
                            color: alpha({fg}, 0.35); }}\n\
         .session-delete:hover {{ background-color: alpha(#ff9e64, 0.18); color: #ff9e64; }}\n\
         .session-name {{ font-size: 15px; font-weight: bold; color: {fg}; }}\n\
         .session-meta {{ color: alpha({fg}, 0.45); font-size: 11px; }}\n\
         .session-badge {{ background-color: alpha({fg}, 0.07); padding: 1px 7px; \
                           border-radius: 0; }}\n\
         .session-badge label {{ font-size: 10px; color: alpha({fg}, 0.6); }}\n\
         .session-badge image {{ color: alpha({fg}, 0.5); }}\n\
         .session-field-label {{ color: alpha({fg}, 0.4); font-size: 10px; \
                                 font-weight: bold; letter-spacing: 1px; margin-top: 8px; }}\n\
         .session-folder-btn {{ background-color: {surface}; background-image: none; \
                                border: 1px solid {border}; border-radius: 0; \
                                padding: 10px 12px; color: {fg}; }}\n\
         .session-folder-btn:hover {{ border-color: #ff9e64; }}\n\
         .session-folder-btn image {{ color: #ff9e64; }}\n\
         .session-count {{ background-color: {surface}; background-image: none; \
                           border: 1px solid {border}; border-radius: 0; padding: 9px; \
                           color: alpha({fg}, 0.8); font-weight: bold; }}\n\
         .session-count:hover {{ background-color: alpha({fg}, 0.06); }}\n\
         .session-count.selected {{ background-color: alpha(#ff9e64, 0.18); \
                                    border-color: #ff9e64; color: #ffb38a; }}\n\
         .session-new {{ background-color: alpha(#ff9e64, 0.16); background-image: none; \
                         color: #ffb38a; \
                         border: 1px solid alpha(#ff9e64, 0.5); border-radius: 0; \
                         padding: 11px; font-weight: bold; margin-top: 10px; }}\n\
         .session-new:hover {{ background-color: alpha(#ff9e64, 0.26); }}\n\
         .session-new:disabled {{ background-color: alpha({fg}, 0.04); background-image: none; \
                                  color: alpha({fg}, 0.3); border-color: alpha({fg}, 0.15); }}\n\
         .session-back {{ background: none; border: none; box-shadow: none; \
                          color: alpha({fg}, 0.55); margin-top: 2px; }}\n\
         .session-back:hover {{ color: {fg}; background-color: alpha({fg}, 0.06); }}\n\
         .session-switcher {{ background: none; border: none; box-shadow: none; \
                              color: {fg}; font-weight: bold; padding: 1px 10px; }}\n\
         .session-switcher:hover {{ background-color: alpha({fg}, 0.08); }}\n\
         .session-popover > contents {{ background-color: {surface}; \
                                        border: 1px solid {border}; border-radius: 0; }}\n\
         .session-menu-row {{ background: none; border: none; box-shadow: none; \
                              border-radius: 0; padding: 4px 10px; color: alpha({fg}, 0.85); }}\n\
         .session-menu-row:hover {{ background-color: alpha({fg}, 0.08); color: {fg}; }}\n\
         .session-menu-row.current {{ color: #ffb38a; }}\n\
         .session-menu-new {{ background: none; border: none; box-shadow: none; \
                              border-radius: 0; padding: 4px 10px; color: #ffb38a; font-weight: bold; }}\n\
         .session-menu-new:hover {{ background-color: alpha(#ff9e64, 0.16); }}\n\
         .view-step {{ min-height: 0; min-width: 26px; padding: 2px 0; \
                       font-weight: bold; }}\n\
         .view-readout {{ color: {fg}; font-size: 12px; }}\n\
         .kbd-header {{ color: alpha({fg}, 0.45); font-size: 10px; font-weight: 700; \
                        letter-spacing: 0.8px; margin-top: 8px; padding-top: 8px; \
                        border-top: 1px solid alpha({fg}, 0.10); }}\n\
         .kbd-row {{ padding: 1px 2px; }}\n\
         .kbd-keys {{ font-family: \"{font}\", monospace; font-size: 12px; \
                      color: alpha({fg}, 0.85); }}\n\
         .kbd-action {{ font-size: 12px; color: alpha({fg}, 0.50); }}"
    );
    let css = scale_css_fonts(&css, scale);
    // Reuse one provider so repeated calls (zoom / font changes) update the
    // stylesheet in place instead of stacking new providers on the display.
    PROVIDER.with(|p| p.load_from_string(&css));
}

thread_local! {
    static PROVIDER: CssProvider = {
        let p = CssProvider::new();
        if let Some(display) = Display::default() {
            gtk4::style_context_add_provider_for_display(
                &display,
                &p,
                STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        p
    };
}
