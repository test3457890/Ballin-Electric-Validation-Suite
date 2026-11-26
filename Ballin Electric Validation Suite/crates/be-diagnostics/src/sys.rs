//! Small, reviewed OS helpers (thread affinity, free space, cache advice).
//! The only FFI in this crate; each call is documented.

use std::path::Path;

/// Pin the calling thread to one logical CPU (Linux). Returns false when
/// unsupported or refused; the caller records reduced coverage.
pub fn pin_current_thread(cpu: usize) -> bool {
    #[cfg(target_os = "linux")]
    {
        if cpu >= libc::CPU_SETSIZE as usize {
            return false;
        }
        // SAFETY: cpu_set_t is a plain bitmask; zeroed is a valid empty set.
        // CPU_SET writes within bounds (checked above). sched_setaffinity(0)
        // affects only the calling thread and reads `set` by pointer.
        unsafe {
            let mut set: libc::cpu_set_t = std::mem::zeroed();
            libc::CPU_SET(cpu, &mut set);
            libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &set) == 0
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cpu;
        false
    }
}

/// Free bytes available to unprivileged users on the volume holding `path`.
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;
        let c = CString::new(path.as_os_str().as_bytes()).ok()?;
        // SAFETY: `c` is a valid NUL-terminated path; `st` is a zeroed POD
        // out-parameter fully written by statvfs on success.
        unsafe {
            let mut st: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c.as_ptr(), &mut st) == 0 {
                return Some((st.f_bavail as u64).saturating_mul(st.f_frsize as u64));
            }
        }
        None
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

/// Ask the kernel to drop cached pages for `file` so read-back reaches the
/// device where possible. Advisory only.
pub fn drop_file_cache(file: &std::fs::File) -> bool {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: valid open fd; posix_fadvise does not access user memory.
        unsafe { libc::posix_fadvise(file.as_raw_fd(), 0, 0, libc::POSIX_FADV_DONTNEED) == 0 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = file;
        false
    }
}

pub fn logical_cpus() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
}
