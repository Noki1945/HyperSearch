use std::path::PathBuf;
pub fn known(id: &windows_sys::core::GUID) -> Option<PathBuf> {
    unsafe {
        let mut path = std::ptr::null_mut();
        if windows_sys::Win32::UI::Shell::SHGetKnownFolderPath(
            id,
            0,
            std::ptr::null_mut(),
            &mut path,
        ) < 0
            || path.is_null()
        {
            return None;
        }
        let mut len = 0;
        while len < 32768 && *path.add(len) != 0 {
            len += 1;
        }
        let value = String::from_utf16_lossy(std::slice::from_raw_parts(path, len));
        windows_sys::Win32::System::Com::CoTaskMemFree(path.cast());
        Some(PathBuf::from(value))
    }
}
pub fn downloads() -> Option<PathBuf> {
    known(&windows_sys::Win32::UI::Shell::FOLDERID_Downloads)
        .or_else(|| std::env::var_os("USERPROFILE").map(|p| PathBuf::from(p).join("Downloads")))
}
