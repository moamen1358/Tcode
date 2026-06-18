//! Colored file-type icons from the Material Icon Theme (MIT) — see
//! `assets/icons/ATTRIBUTION.txt`. The SVGs are embedded at build time, written
//! to a cache dir at startup, and chosen per file by name/extension, giving the
//! sidebar Zed/VSCode-style icons independent of the system icon theme.

use std::fs;
use std::path::{Path, PathBuf};

macro_rules! icon {
    ($name:literal) => {
        ($name, include_str!(concat!("../assets/icons/", $name, ".svg")))
    };
}

// (icon-name, svg-content). Folder + a broad set of language/file icons.
const ICONS: &[(&str, &str)] = &[
    icon!("folder-base"),
    icon!("document"),
    icon!("rust"),
    icon!("toml"),
    icon!("markdown"),
    icon!("json"),
    icon!("lock"),
    icon!("docker"),
    icon!("git"),
    icon!("console"),
    icon!("yaml"),
    icon!("xml"),
    icon!("html"),
    icon!("css"),
    icon!("sass"),
    icon!("javascript"),
    icon!("typescript"),
    icon!("react"),
    icon!("python"),
    icon!("ruby"),
    icon!("go"),
    icon!("java"),
    icon!("c"),
    icon!("cpp"),
    icon!("csharp"),
    icon!("php"),
    icon!("swift"),
    icon!("lua"),
    icon!("nodejs"),
    icon!("npm"),
    icon!("settings"),
    icon!("makefile"),
    icon!("readme"),
    icon!("license"),
    icon!("image"),
    icon!("svg"),
    icon!("video"),
    icon!("audio"),
    icon!("zip"),
    icon!("pdf"),
    icon!("font"),
    icon!("database"),
    icon!("key"),
];

/// Convert a hex color (the digits after `#`) to its luminance-equivalent gray.
fn gray_of(hex: &str) -> Option<String> {
    let (r, g, b) = match hex.len() {
        3 | 4 => (
            u8::from_str_radix(&hex[0..1], 16).ok()? * 17,
            u8::from_str_radix(&hex[1..2], 16).ok()? * 17,
            u8::from_str_radix(&hex[2..3], 16).ok()? * 17,
        ),
        6 | 8 => (
            u8::from_str_radix(&hex[0..2], 16).ok()?,
            u8::from_str_radix(&hex[2..4], 16).ok()?,
            u8::from_str_radix(&hex[4..6], 16).ok()?,
        ),
        _ => return None,
    };
    let lum = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32).round() as u8;
    Some(format!("#{lum:02x}{lum:02x}{lum:02x}"))
}

/// Desaturate every fill/stroke color in an SVG to a luminance gray, so the
/// icons render as simple monochrome (shapes kept, color removed). `url(#id)`
/// references are left untouched.
fn grayscale(svg: &str) -> String {
    let s = svg.as_bytes();
    let mut out = String::with_capacity(svg.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'#' && (i == 0 || s[i - 1] != b'(') {
            let mut j = i + 1;
            while j < s.len() && s[j].is_ascii_hexdigit() {
                j += 1;
            }
            let digits = &svg[i + 1..j];
            if matches!(digits.len(), 3 | 4 | 6 | 8) {
                if let Some(gray) = gray_of(digits) {
                    out.push_str(&gray);
                    i = j;
                    continue;
                }
            }
        }
        out.push(s[i] as char);
        i += 1;
    }
    out
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".cache")
        });
    base.join("tessera").join("icons")
}

/// Write the embedded icons to the cache dir (idempotent). Returns that dir.
pub fn ensure() -> PathBuf {
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    for (name, svg) in ICONS {
        let _ = fs::write(dir.join(format!("{name}.svg")), grayscale(svg));
    }
    dir
}

/// The bundled icon name for a file/folder (matched by exact name, then extension).
fn icon_name(name: &str, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder-base";
    }
    let lower = name.to_lowercase();
    match lower.as_str() {
        "dockerfile" | ".dockerignore" => return "docker",
        "makefile" => return "makefile",
        "package.json" => return "nodejs",
        "package-lock.json" | ".npmrc" | "yarn.lock" => return "npm",
        "cargo.lock" => return "lock",
        _ => {}
    }
    if lower.starts_with(".git") {
        return "git";
    }
    if lower.starts_with("readme") {
        return "readme";
    }
    if lower.starts_with("license") || lower.starts_with("licence") {
        return "license";
    }
    if lower.starts_with("docker-compose") {
        return "docker";
    }
    let ext = match lower.rsplit_once('.') {
        Some((_, e)) => e,
        None => "",
    };
    match ext {
        "rs" => "rust",
        "toml" => "toml",
        "md" | "markdown" => "markdown",
        "json" | "json5" => "json",
        "lock" => "lock",
        "yml" | "yaml" => "yaml",
        "xml" => "xml",
        "html" | "htm" => "html",
        "css" => "css",
        "scss" | "sass" => "sass",
        "js" | "cjs" | "mjs" => "javascript",
        "ts" => "typescript",
        "tsx" | "jsx" => "react",
        "py" => "python",
        "rb" => "ruby",
        "go" => "go",
        "java" => "java",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => "cpp",
        "cs" => "csharp",
        "php" => "php",
        "swift" => "swift",
        "lua" => "lua",
        "sh" | "bash" | "zsh" | "fish" => "console",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" => "image",
        "svg" => "svg",
        "mp4" | "mkv" | "mov" | "webm" | "avi" => "video",
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => "audio",
        "zip" | "tar" | "gz" | "xz" | "7z" | "bz2" | "zst" => "zip",
        "pdf" => "pdf",
        "ttf" | "otf" | "woff" | "woff2" => "font",
        "db" | "sqlite" | "sqlite3" | "sql" => "database",
        "pem" | "key" | "crt" | "cert" => "key",
        "ini" | "cfg" | "conf" | "config" | "editorconfig" | "lock-cfg" => "settings",
        _ => "document",
    }
}

/// Path to the bundled icon for `name` within icon dir `dir`.
pub fn icon_path(dir: &Path, name: &str, is_dir: bool) -> PathBuf {
    dir.join(format!("{}.svg", icon_name(name, is_dir)))
}
