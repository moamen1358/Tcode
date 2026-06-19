//! Colored file-type icons from the Material Icon Theme (MIT) — see
//! `assets/icons/ATTRIBUTION.txt`. The SVGs are embedded at build time, written
//! to a cache dir at startup, and chosen per file by name/extension, giving the
//! sidebar Zed/VSCode-style icons independent of the system icon theme.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use gtk4::gdk::Texture;
use gtk4::gdk_pixbuf::Pixbuf;

macro_rules! icon {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!("../assets/icons/", $name, ".svg")),
        )
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

fn cache_dir() -> PathBuf {
    gtk4::glib::user_cache_dir().join("tessera").join("icons")
}

/// Write the embedded icons to the cache dir (idempotent). Returns that dir.
/// The SVGs are written verbatim so they keep their original Material colors.
pub fn ensure() -> PathBuf {
    let dir = cache_dir();
    let _ = fs::create_dir_all(&dir);
    for (name, svg) in ICONS {
        let _ = fs::write(dir.join(format!("{name}.svg")), svg);
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

thread_local! {
    /// Cache of rasterized icon textures, keyed by (icon name, device px). Only
    /// ~43 icons at a couple of sizes ever land here, so it stays tiny.
    static TEXTURES: RefCell<HashMap<(&'static str, i32), Texture>> =
        RefCell::new(HashMap::new());
}

/// A crisp icon texture for `name`, rasterized by librsvg at exactly `px` device
/// pixels (then cached). Rendering the *vector* at the target size — rather than
/// letting GtkImage scale a fixed natural-size bitmap — keeps the icon sharp at
/// any DPI. Returns `None` only if the SVG fails to load.
pub fn icon_texture(dir: &Path, name: &str, is_dir: bool, px: i32) -> Option<Texture> {
    let icon = icon_name(name, is_dir);
    let px = px.max(1);
    if let Some(tex) = TEXTURES.with(|c| c.borrow().get(&(icon, px)).cloned()) {
        return Some(tex);
    }
    let path = dir.join(format!("{icon}.svg"));
    let pb = Pixbuf::from_file_at_scale(&path, px, px, true).ok()?;
    let tex = Texture::for_pixbuf(&pb);
    TEXTURES.with(|c| c.borrow_mut().insert((icon, px), tex.clone()));
    Some(tex)
}
