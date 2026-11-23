use std::process::Child;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

use crate::core::{BackendError, Res};

pub(crate) struct ProcessJob(HANDLE);

impl ProcessJob {
    pub(crate) fn new() -> Res<Self> {
        let job = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|error| {
            BackendError::new("SPAWN_FAILED", format!("无法创建进程任务对象: {error}"))
        })?;
        let job = Self(job);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const _,
                std::mem::size_of_val(&limits) as u32,
            )
        }
        .map_err(|error| {
            BackendError::new("SPAWN_FAILED", format!("无法配置进程树清理: {error}"))
        })?;
        Ok(job)
    }

    pub(crate) fn assign(&self, child: &Child) -> Res<()> {
        use std::os::windows::io::AsRawHandle;
        unsafe { AssignProcessToJobObject(self.0, HANDLE(child.as_raw_handle())) }.map_err(
            |error| BackendError::new("SPAWN_FAILED", format!("无法隔离后端进程树: {error}")),
        )
    }
}

impl Drop for ProcessJob {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}
