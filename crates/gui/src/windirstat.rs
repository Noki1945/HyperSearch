//! External application discovery. No recursive disk scans, installer execution or startup launch.
use std::{
    io,
    path::{Path, PathBuf},
};
pub const RELEASES: &str = "https://github.com/windirstat/windirstat/releases/latest";
pub const WEBSITE: &str = "https://windirstat.net/";
pub fn state_file() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("HyperSearch/windirstat.path")
}
fn valid(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("windirstat.exe"))
        && path.is_file()
}
pub fn remember(path: &Path, state: &Path) -> io::Result<()> {
    if !valid(path) {
        return Err(io::Error::other("Укажите существующий windirstat.exe"));
    }
    let path = path.canonicalize()?;
    if let Some(parent) = state.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(state, path.to_string_lossy().as_bytes())
}
pub fn discover(state: &Path, candidates: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    if let Ok(saved) = std::fs::read_to_string(state) {
        let p = PathBuf::from(saved.trim());
        if valid(&p) {
            return Some(p);
        }
    }
    for path in candidates {
        if valid(&path) {
            let _ = remember(&path, state);
            return Some(path);
        }
    }
    None
}
fn registry_path(
    root: windows_sys::Win32::System::Registry::HKEY,
    key: &str,
    value: &str,
) -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::*;
    let key: Vec<_> = key.encode_utf16().chain(Some(0)).collect();
    let value: Vec<_> = value.encode_utf16().chain(Some(0)).collect();
    for flag in [RRF_SUBKEY_WOW6464KEY, RRF_SUBKEY_WOW6432KEY] {
        let mut buffer = vec![0u16; 32768];
        let mut bytes = (buffer.len() * 2) as u32;
        let result = unsafe {
            RegGetValueW(
                root,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_SZ | flag,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        if result == 0 {
            let n = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            return Some(PathBuf::from(
                String::from_utf16_lossy(&buffer[..n]).trim_matches('"'),
            ));
        }
    }
    None
}
pub fn installed() -> Option<PathBuf> {
    use windows_sys::Win32::System::Registry::*;
    let mut candidates = Vec::new();
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Some(p) = registry_path(
            root,
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\windirstat.exe",
            "",
        ) {
            candidates.push(p);
        }
        for key in [
            "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall\\WinDirStat",
            "SOFTWARE\\WinDirStat",
        ] {
            if let Some(p) = registry_path(root, key, "InstallLocation") {
                candidates.push(p.join("windirstat.exe"));
            }
        }
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"] {
        if let Some(p) = std::env::var_os(var) {
            let p = PathBuf::from(p);
            candidates.push(p.join("WinDirStat/windirstat.exe"));
            candidates.push(p.join("Programs/WinDirStat/windirstat.exe"));
        }
    }
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        let p = PathBuf::from(p);
        candidates.push(p.join("Microsoft/WinGet/Links/windirstat.exe"));
        candidates.push(p.join("HyperSearch/Tools/WinDirStat/windirstat.exe"));
    }
    if let Some(p) = std::env::var_os("USERPROFILE") {
        let downloads = PathBuf::from(p).join("Downloads");
        candidates.push(downloads.join("windirstat.exe"));
        candidates.push(downloads.join("WinDirStat/windirstat.exe"));
        // Bounded discovery of extracted official portable packages; never a recursive scan.
        if let Ok(entries) = std::fs::read_dir(&downloads) {
            for entry in entries.take(256).flatten() {
                if entry
                    .file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .starts_with("windirstat")
                {
                    candidates.push(entry.path().join("windirstat.exe"));
                    candidates.push(entry.path().join("x64/windirstat.exe"));
                }
            }
        }
    }
    discover(&state_file(), candidates)
}
/// Remembers packages opened through settings. Never executes an unverified installer.
pub fn downloaded() -> Option<PathBuf> {
    let state = state_file().with_file_name("windirstat.download");
    let downloads = PathBuf::from(std::env::var_os("USERPROFILE")?).join("Downloads");
    downloaded_from(&state, &downloads)
}
fn downloaded_from(state: &Path, downloads: &Path) -> Option<PathBuf> {
    if let Ok(saved) = std::fs::read_to_string(state) {
        let path = PathBuf::from(saved.trim());
        if path.is_file() {
            return Some(path);
        }
    }
    let mut packages: Vec<_> = std::fs::read_dir(downloads)
        .ok()?
        .take(256)
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name().is_some_and(|name| {
                name.to_string_lossy()
                    .to_lowercase()
                    .starts_with("windirstat")
            }) && path.extension().is_some_and(|ext| {
                matches!(
                    ext.to_string_lossy().to_lowercase().as_str(),
                    "zip" | "msi" | "exe"
                )
            }) && path.is_file()
        })
        .collect();
    packages.sort_by_key(|path| {
        std::cmp::Reverse(path.metadata().ok().and_then(|m| m.modified().ok()))
    });
    let path = packages.into_iter().next()?;
    let _ = std::fs::write(state, path.to_string_lossy().as_bytes());
    Some(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn downloaded_package_is_remembered_without_execution() {
        let dir = std::env::temp_dir().join(format!("hs-wds-package-{}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let state = dir.join("download.path");
        let zip = dir.join("WinDirStat-x64.zip");
        std::fs::write(&zip, b"test fixture, not an executable").unwrap();
        assert_eq!(downloaded_from(&state, &dir), Some(zip.clone()));
        assert_eq!(
            downloaded_from(&state, &dir.join("missing")),
            Some(zip.clone())
        );
        std::fs::remove_file(&zip).unwrap();
        assert!(downloaded_from(&state, &dir).is_none());
        std::fs::remove_file(state).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn remembers_existing_and_recovers_missing() {
        let dir = std::env::temp_dir().join(format!("hs-wds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("windirstat.exe");
        let state = dir.join("remembered.path");
        std::fs::write(&exe, b"test fixture, never executed").unwrap();
        assert_eq!(discover(&state, [exe.clone()]).unwrap(), exe);
        assert!(discover(&state, []).is_some());
        std::fs::remove_file(&exe).unwrap();
        assert!(discover(&state, []).is_none());
        assert!(remember(&exe, &state).is_err());
        std::fs::remove_file(state).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
