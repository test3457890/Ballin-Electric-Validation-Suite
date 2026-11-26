//! Windows Job Object containment for worker processes.
//!
//! Each worker is assigned to a job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`,
//! so the worker and any process it creates are terminated when the job
//! handle closes: on explicit abort, on supervisor drop, or if the
//! supervisor itself crashes (the OS closes its handles).

use std::os::windows::io::AsRawHandle;
use std::process::Child;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};

pub struct Job(HANDLE);

// SAFETY: a job handle may be used from any thread.
unsafe impl Send for Job {}

impl Job {
    /// Create a kill-on-close job and assign `child` to it.
    pub fn contain(child: &Child) -> std::io::Result<Job> {
        // SAFETY: null attributes/name create an anonymous job; the returned
        // handle is checked and owned by `Job` (closed exactly once in Drop).
        let h = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if h.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Job(h);
        // SAFETY: zeroed is a valid initial value for this plain C struct;
        // we pass its exact size and a valid job handle.
        unsafe {
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(std::io::Error::last_os_error());
            }
            // The child handle is valid while `child` is alive (not yet waited).
            if AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
        Ok(job)
    }

    /// Terminate every process in the job.
    pub fn terminate(&self) {
        // SAFETY: valid job handle owned by self.
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // SAFETY: handle owned by self, closed once. Kill-on-close then
        // terminates anything still running in the job.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
