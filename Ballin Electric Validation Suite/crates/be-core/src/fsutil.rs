//! Defensive file helpers shared by all front ends.
//!
//! * `read_limited` bounds the size of operator-supplied files (profiles,
//!   safety profiles, manifests, BOMs) so a malicious or corrupted file
//!   cannot exhaust memory.
//! * `write_atomic` writes through a uniquely named temporary file created
//!   with `create_new` (never following a pre-planted file or symlink) and
//!   renames it into place, so readers never see partial output.
//! * `trusted_tool` resolves external programs from fixed system locations
//!   instead of `PATH`, preventing PATH hijacking when run elevated.

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Default limit for profile / manifest style inputs.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

pub fn read_limited(path: &Path, max: u64) -> io::Result<String> {
    let f = std::fs::File::open(path)?;
    let meta = f.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if meta.len() > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds {max} byte limit"),
        ));
    }
    let mut s = String::new();
    f.take(max + 1).read_to_string(&mut s)?;
    if s.len() as u64 > max {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file exceeds {max} byte limit"),
        ));
    }
    Ok(s)
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("out");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = dir.join(format!(
        ".{name}.{}.{}.{}.tmp",
        std::process::id(),
        nanos,
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Create (if needed) a directory readable only by the current user.
pub fn private_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "refusing non-directory or symlink",
            ));
        }
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Per-user application data directory (offline, never shared with other users).
pub fn user_data_dir() -> PathBuf {
    #[cfg(windows)]
    {
        std::env::var("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("BE Validation")
    }
    #[cfg(not(windows))]
    {
        std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|_| std::env::var("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("be-validation")
    }
}

/// Locate an external tool in fixed system directories only.
pub fn trusted_tool(name: &str) -> Option<PathBuf> {
    let candidates: Vec<PathBuf> = if cfg!(windows) {
        let root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
        let pf = std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".into());
        match name {
            "powershell" => {
                vec![PathBuf::from(&root).join(r"System32\WindowsPowerShell\v1.0\powershell.exe")]
            }
            "smartctl" => vec![PathBuf::from(&pf).join(r"smartmontools\bin\smartctl.exe")],
            "explorer" => vec![PathBuf::from(&root).join("explorer.exe")],
            _ => vec![],
        }
    } else {
        [
            "/usr/sbin",
            "/usr/bin",
            "/sbin",
            "/bin",
            "/usr/local/sbin",
            "/usr/local/bin",
        ]
        .iter()
        .map(|d| Path::new(d).join(name))
        .collect()
    };
    candidates.into_iter().find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limited_read_rejects_large_and_non_files() {
        let d = tempfile_dir();
        let p = d.join("big.json");
        std::fs::write(&p, vec![b'a'; 2000]).unwrap();
        assert!(read_limited(&p, 1000).is_err());
        assert_eq!(read_limited(&p, 4000).unwrap().len(), 2000);
        assert!(read_limited(&d, 4000).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn atomic_write_replaces_and_leaves_no_temp() {
        let d = tempfile_dir();
        let p = d.join("out.txt");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "two");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    fn tempfile_dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "be-core-test-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        d
    }
}
