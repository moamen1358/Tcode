//! Small filesystem helpers shared across Tcode (no GTK).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Atomically write `bytes` to `path`.
///
/// Writes a freshly-created sibling temp file (opened `O_CREAT | O_EXCL`, so a
/// pre-planted file or symlink at the temp path is rejected rather than followed),
/// flushes it to disk, then `rename`s it over `path` — an atomic swap on the same
/// filesystem. A crash or power loss therefore leaves *either* the previous file
/// intact *or* the new one complete, never a half-written/truncated file. The
/// rename also replaces a symlink at `path` with the real file instead of writing
/// through it to the link target.
///
/// On Unix the final file's permission bits are set to exactly `mode` (the temp is
/// chmod-ed before the rename, so `umask` can't loosen/clip it, and there is no
/// post-rename window where the target exists with the wrong bits).
pub fn atomic_write(path: &Path, bytes: &[u8], mode: u32) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = temp_sibling(path);

    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true); // O_CREAT | O_EXCL — never follow/overwrite
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(mode);
    }
    #[cfg(not(unix))]
    let _ = mode;

    let written = (|| -> std::io::Result<()> {
        let file = opts.open(&tmp)?;
        let mut file = file;
        file.write_all(bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // O_CREAT's mode is masked by umask; set the exact bits now. Safe: the
            // temp is private (unique name, O_EXCL) and not yet the target.
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        file.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// A unique temp path beside `path` (same directory ⇒ same filesystem ⇒ the final
/// `rename` is atomic). Uniqueness from pid + monotonic-ish clock + an in-process
/// counter, so concurrent writers — including separate `NON_UNIQUE` launches — never
/// collide on the `O_EXCL` create.
fn temp_sibling(path: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    let mut name = path.file_name().map(|s| s.to_os_string()).unwrap_or_default();
    name.push(format!(".tcode-tmp.{pid:x}.{nanos:x}.{seq:x}"));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_then_overwrites_atomically() {
        let dir = std::env::temp_dir().join(format!("tcode-fsutil-{}", std::process::id()));
        let path = dir.join("data.bin");
        atomic_write(&path, b"first", 0o600).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        // Overwriting leaves no temp files behind and fully replaces the content.
        atomic_write(&path, b"second-longer", 0o600).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second-longer");
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("tcode-tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp files left behind: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn sets_exact_mode_regardless_of_umask() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("tcode-fsutil-mode-{}", std::process::id()));
        let path = dir.join("secret");
        atomic_write(&path, b"x", 0o600).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "expected 0o600, got {mode:o}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
