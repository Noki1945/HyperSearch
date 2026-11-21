//! Process metrics, separate from estimated index allocations.
#[derive(Debug)]
pub struct Metrics {
    pub working_set_bytes: usize,
    pub private_bytes: usize,
    pub cpu_seconds: f64,
}
#[cfg(windows)]
pub fn process() -> Option<Metrics> {
    use std::ffi::c_void;
    #[repr(C)]
    #[derive(Default)]
    struct Memory {
        cb: u32,
        faults: u32,
        peak: usize,
        working: usize,
        peak_pool: usize,
        pool: usize,
        peak_nonpaged: usize,
        nonpaged: usize,
        commit: usize,
        peak_commit: usize,
        private: usize,
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(h: *mut c_void, m: *mut Memory, size: u32) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn GetProcessTimes(
            h: *mut c_void,
            created: *mut u64,
            exit: *mut u64,
            kernel: *mut u64,
            user: *mut u64,
        ) -> i32;
    }
    let mut m = Memory {
        cb: std::mem::size_of::<Memory>() as u32,
        ..Default::default()
    };
    let (mut c, mut e, mut k, mut u) = (0, 0, 0, 0);
    unsafe {
        let h = GetCurrentProcess();
        if GetProcessMemoryInfo(h, &mut m, std::mem::size_of::<Memory>() as u32) == 0
            || GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) == 0
        {
            return None;
        }
    }
    Some(Metrics {
        working_set_bytes: m.working,
        private_bytes: m.private,
        cpu_seconds: (k + u) as f64 / 10_000_000.,
    })
}
#[cfg(not(windows))]
pub fn process() -> Option<Metrics> {
    None
}
