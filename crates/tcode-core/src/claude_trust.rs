//! Pre-accept Claude Code's per-folder "workspace trust" dialog so agent panes
//! launch straight into a ready session instead of stopping on
//! *"Quick safety check: Is this a project you created or one you trust?"*.
//!
//! Claude gates each new working directory behind that dialog; `--dangerously-
//! skip-permissions` skips per-action permission checks but **not** this trust
//! check. There is no flag, env var, or settings key to disable it (the whole
//! `CLAUDE_CODE_*` env surface was audited — none touches trust). Trust is stored
//! per folder in `~/.claude.json` under `projects.<key>.hasTrustDialogAccepted`,
//! and setting it is exactly what clicking *"Yes, I trust this folder"* does.
//!
//! So when tcode opens a session it pre-accepts trust for that folder here,
//! mirroring Claude's own writer: the key is `path.normalize(path.resolve(cwd))`
//! (absolute, lexically cleaned, **no** symlink resolution), a new entry is the
//! default project config with the flag set, and an existing entry keeps every
//! other field. It's idempotent (already-trusted → no write) and entirely
//! best-effort: a missing file, parse error, or unexpected shape leaves
//! `~/.claude.json` untouched and Claude just shows its dialog as before.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

/// Pre-accept Claude's workspace-trust dialog for `folder` (the session root we're
/// about to launch Claude panes in). Best-effort and idempotent — see module docs.
pub fn ensure_trusted(folder: &Path) {
    if let Some(path) = claude_json_path() {
        ensure_trusted_in(&path, folder);
    }
}

/// `~/.claude.json` — Claude Code's global config file (distinct from the
/// `~/.claude/` directory). `None` if `HOME` is unset.
fn claude_json_path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude.json"))
}

/// Set `projects.<key>.hasTrustDialogAccepted = true` in `claude_json` for
/// `folder`, preserving everything else. Factored out from [`ensure_trusted`] so
/// it can be tested against a temp file. Silent on every error path.
pub fn ensure_trusted_in(claude_json: &Path, folder: &Path) {
    let key = normalize_key(folder);

    // No config yet → let Claude create it and prompt as usual; never create a
    // stub ourselves (Claude's first run writes onboarding state we'd clobber).
    let Ok(text) = std::fs::read_to_string(claude_json) else {
        return;
    };
    // Unparseable or not a JSON object → never risk corrupting the user's config.
    let Ok(mut root) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let Some(obj) = root.as_object_mut() else {
        return;
    };

    // Find or create the `projects` map; bail if it's present but not an object.
    let projects = obj
        .entry("projects")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(projects) = projects.as_object_mut() else {
        return;
    };

    // Idempotent: already trusted → nothing to do (and crucially, no rewrite of
    // the user's 80 KB config on every session open).
    if projects
        .get(&key)
        .and_then(|p| p.get("hasTrustDialogAccepted"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        return;
    }

    // Flip just the flag on an existing entry (keeping its allowedTools, mcp
    // settings, …); otherwise insert a fresh default entry with trust set. The
    // get_mut borrow is fully contained in this match (it yields a bool), so the
    // later `projects.insert` is a clean reborrow.
    let needs_new_entry = match projects.get_mut(&key).and_then(Value::as_object_mut) {
        Some(entry) => {
            entry.insert("hasTrustDialogAccepted".into(), Value::Bool(true));
            false
        }
        None => true,
    };
    if needs_new_entry {
        projects.insert(key, default_trusted_project());
    }

    // Match Claude's file: 2-space pretty-print, owner-only (0o600), atomic.
    if let Ok(out) = serde_json::to_string_pretty(&root) {
        let _ = crate::fsutil::atomic_write(claude_json, out.as_bytes(), 0o600);
    }
}

/// A new project entry mirroring Claude's default project config (`wze` in the
/// bundle) with the trust flag pre-set, so Claude reads a well-formed entry.
fn default_trusted_project() -> Value {
    serde_json::json!({
        "allowedTools": [],
        "mcpContextUris": [],
        "mcpServers": {},
        "enabledMcpjsonServers": [],
        "disabledMcpjsonServers": [],
        "hasTrustDialogAccepted": true,
        "projectOnboardingSeenCount": 0,
        "hasClaudeMdExternalIncludesApproved": false,
        "hasClaudeMdExternalIncludesWarningShown": false
    })
}

/// The `~/.claude.json` projects key for `folder`, matching Claude's
/// `tq(path.resolve(cwd))` = `path.normalize(path.resolve(folder))`: make it
/// absolute (against the process cwd if needed), then lexically collapse `//`,
/// `.`, and `..` and drop the trailing slash. No filesystem access, so — like
/// `path.resolve`/`normalize` — symlinks are left unresolved.
fn normalize_key(folder: &Path) -> String {
    let abs = if folder.is_absolute() {
        folder.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("/"))
            .join(folder)
    };
    let s = abs.to_string_lossy();
    let mut out: Vec<&str> = Vec::new();
    for seg in s.split('/') {
        match seg {
            "" | "." => {}           // collapse `//` and `.`
            ".." => {
                out.pop(); // up one; can't rise above root
            }
            seg => out.push(seg),
        }
    }
    format!("/{}", out.join("/")) // always absolute; root collapses to "/"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn tmp_json(tag: &str) -> PathBuf {
        // Unique per test (pid + tag + sequence) so parallel tests never collide.
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "tcode-trust-{}-{tag}-{seq}.json",
            std::process::id()
        ))
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn trusted(v: &Value, key: &str) -> bool {
        v["projects"][key]["hasTrustDialogAccepted"] == Value::Bool(true)
    }

    #[test]
    fn normalize_key_cases() {
        assert_eq!(normalize_key(Path::new("/home/m/llama.cpp")), "/home/m/llama.cpp");
        assert_eq!(normalize_key(Path::new("/foo/bar/")), "/foo/bar"); // trailing slash dropped
        assert_eq!(normalize_key(Path::new("/foo//bar")), "/foo/bar"); // double slash collapsed
        assert_eq!(normalize_key(Path::new("/foo/./bar")), "/foo/bar"); // `.` collapsed
        assert_eq!(normalize_key(Path::new("/foo/baz/../bar")), "/foo/bar"); // `..` resolved
        assert_eq!(normalize_key(Path::new("/a/../../b")), "/b"); // can't rise above root
        assert_eq!(normalize_key(Path::new("/")), "/"); // root
    }

    #[test]
    fn creates_entry_for_new_folder() {
        let path = tmp_json("new");
        std::fs::write(&path, r#"{"projects":{}}"#).unwrap();
        ensure_trusted_in(&path, Path::new("/foo/bar"));
        assert!(trusted(&read(&path), "/foo/bar"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn creates_projects_map_when_absent_and_keeps_top_level() {
        let path = tmp_json("nomap");
        std::fs::write(&path, r#"{"numStartups":5,"oauthAccount":{"k":1}}"#).unwrap();
        ensure_trusted_in(&path, Path::new("/a"));
        let v = read(&path);
        assert_eq!(v["numStartups"], Value::from(5)); // top-level field preserved
        assert_eq!(v["oauthAccount"]["k"], Value::from(1));
        assert!(trusted(&v, "/a"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn preserves_other_projects_and_their_fields() {
        let path = tmp_json("others");
        std::fs::write(
            &path,
            r#"{"projects":{"/other":{"hasTrustDialogAccepted":true,"allowedTools":["X"]}}}"#,
        )
        .unwrap();
        ensure_trusted_in(&path, Path::new("/new"));
        let v = read(&path);
        // Untouched neighbour keeps its fields…
        assert!(trusted(&v, "/other"));
        assert_eq!(v["projects"]["/other"]["allowedTools"][0], Value::from("X"));
        // …and the new folder is trusted.
        assert!(trusted(&v, "/new"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn flips_existing_untrusted_entry_keeping_fields() {
        let path = tmp_json("flip");
        std::fs::write(
            &path,
            r#"{"projects":{"/p":{"hasTrustDialogAccepted":false,"allowedTools":["keep"],"projectOnboardingSeenCount":3}}}"#,
        )
        .unwrap();
        ensure_trusted_in(&path, Path::new("/p"));
        let v = read(&path);
        assert!(trusted(&v, "/p")); // flipped true
        assert_eq!(v["projects"]["/p"]["allowedTools"][0], Value::from("keep")); // kept
        assert_eq!(v["projects"]["/p"]["projectOnboardingSeenCount"], Value::from(3)); // kept
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn idempotent_does_not_rewrite_when_already_trusted() {
        let path = tmp_json("idem");
        // Deliberately compact (non-canonical) so any rewrite would reformat it.
        let original = r#"{"projects":{"/p":{"hasTrustDialogAccepted":true}}}"#;
        std::fs::write(&path, original).unwrap();
        ensure_trusted_in(&path, Path::new("/p"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original); // byte-identical
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn normalizes_path_before_keying() {
        let path = tmp_json("norm");
        std::fs::write(&path, r#"{"projects":{}}"#).unwrap();
        ensure_trusted_in(&path, Path::new("/foo/bar/")); // trailing slash
        let v = read(&path);
        assert!(trusted(&v, "/foo/bar")); // stored without the trailing slash
        assert!(v["projects"].get("/foo/bar/").is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_is_noop() {
        let path = tmp_json("missing");
        let _ = std::fs::remove_file(&path); // ensure absent
        ensure_trusted_in(&path, Path::new("/x")); // must not panic
        assert!(!path.exists()); // and must not create it
    }

    #[test]
    fn non_object_or_unparseable_left_untouched() {
        for (tag, body) in [("array", "[1,2,3]"), ("garbage", "not json {{")] {
            let path = tmp_json(tag);
            std::fs::write(&path, body).unwrap();
            ensure_trusted_in(&path, Path::new("/x"));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), body); // unchanged
            let _ = std::fs::remove_file(&path);
        }
    }

    #[test]
    fn preserves_exact_number_text_of_untouched_fields() {
        // Claude's config carries high-precision telemetry floats written by JS;
        // arbitrary_precision must keep their exact digits (not re-spell to the
        // same f64), so we disturb the file as little as possible.
        let path = tmp_json("precision");
        std::fs::write(
            &path,
            r#"{"stat":180.66666666666666,"tiny":0.09999999999999787,"projects":{}}"#,
        )
        .unwrap();
        ensure_trusted_in(&path, Path::new("/x"));
        let out = std::fs::read_to_string(&path).unwrap();
        assert!(out.contains("180.66666666666666"), "float re-spelled: {out}");
        assert!(out.contains("0.09999999999999787"), "float re-spelled: {out}");
        assert!(trusted(&read(&path), "/x"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn projects_present_but_not_object_is_left_untouched() {
        let path = tmp_json("badprojects");
        let body = r#"{"projects":"oops"}"#;
        std::fs::write(&path, body).unwrap();
        ensure_trusted_in(&path, Path::new("/x"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), body); // unchanged
        let _ = std::fs::remove_file(&path);
    }
}
