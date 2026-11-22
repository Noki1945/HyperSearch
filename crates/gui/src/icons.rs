//! Shell type icons only: no per-file executable/image extraction or unbounded cache.
use std::collections::HashMap;
use windows_sys::Win32::UI::Shell::*;
pub fn key(name: &str, directory: bool) -> String {
    if directory {
        "<folder>".into()
    } else {
        std::path::Path::new(name)
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }
}
pub struct Icons {
    pub small: usize,
    pub large: usize,
    pub indices: HashMap<String, i32>,
}
pub fn load(keys: Vec<String>) -> Icons {
    use windows_sys::Win32::System::Com::*;
    let initialized =
        unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0;
    let mut result = Icons {
        small: 0,
        large: 0,
        indices: HashMap::new(),
    };
    for key in keys.into_iter().take(256) {
        if result.indices.contains_key(&key) {
            continue;
        }
        let folder = key == "<folder>";
        let name: Vec<u16> = (if folder {
            "folder".into()
        } else {
            format!("file.{key}")
        })
        .encode_utf16()
        .chain(Some(0))
        .collect();
        let mut info: SHFILEINFOW = unsafe { std::mem::zeroed() };
        let attributes = if folder { 0x10 } else { 0x80 };
        unsafe {
            let small = SHGetFileInfoW(
                name.as_ptr(),
                attributes,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES | SHGFI_SMALLICON,
            );
            if small != 0 {
                result.small = small;
                result.indices.insert(key, info.iIcon);
            }
            let large = SHGetFileInfoW(
                name.as_ptr(),
                attributes,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX | SHGFI_USEFILEATTRIBUTES | SHGFI_LARGEICON,
            );
            if large != 0 {
                result.large = large;
            }
        }
    }
    if initialized {
        unsafe {
            CoUninitialize();
        }
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_type_icons_without_file_access() {
        assert_eq!(key("Отчёт.XLSX", false), "xlsx");
        let icons = load(vec![key("", true), "txt".into(), "exe".into()]);
        assert_ne!(icons.small, 0);
        assert_ne!(icons.large, 0);
        assert!(icons.indices.contains_key("<folder>"));
        assert!(icons.indices.values().all(|&i| i >= 0));
    }
}
