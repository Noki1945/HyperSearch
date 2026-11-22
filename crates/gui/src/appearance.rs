//! Event-driven system backdrop, with opaque Windows 10 fallback.
use std::{io, path::Path};
use windows_sys::Win32::Foundation::HWND;
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Theme {
    System,
    Light,
    Dark,
}
#[derive(Clone, Copy, Debug)]
pub struct Appearance {
    pub theme: Theme,
    pub transparency: bool,
    pub animations: bool,
    pub font_percent: u32,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            transparency: true,
            animations: false,
            font_percent: 100,
        }
    }
}
impl Appearance {
    pub fn load(path: &Path) -> Self {
        let Ok(s) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        let mut lines = s.lines();
        Self {
            theme: match lines.next() {
                Some("light") => Theme::Light,
                Some("dark") => Theme::Dark,
                _ => Theme::System,
            },
            transparency: lines.next() != Some("opaque"),
            animations: lines.next() == Some("animated"),
            font_percent: lines
                .next()
                .and_then(|s| s.parse().ok())
                .filter(|n| [90, 100, 110, 120].contains(n))
                .unwrap_or(100),
        }
    }
    pub fn save(self, path: &Path) -> io::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(
            path,
            format!(
                "{}\n{}\n{}\n{}\n",
                match self.theme {
                    Theme::System => "system",
                    Theme::Light => "light",
                    Theme::Dark => "dark",
                },
                if self.transparency { "glass" } else { "opaque" },
                if self.animations { "animated" } else { "still" },
                self.font_percent
            ),
        )
    }
    pub fn dark(self) -> bool {
        match self.theme {
            Theme::Dark => true,
            Theme::Light => false,
            Theme::System => {
                use windows_sys::Win32::System::Registry::*;
                let k: Vec<_> = "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"
                    .encode_utf16()
                    .chain(Some(0))
                    .collect();
                let v: Vec<_> = "AppsUseLightTheme".encode_utf16().chain(Some(0)).collect();
                let mut value = 1u32;
                let mut len = 4;
                unsafe {
                    RegGetValueW(
                        HKEY_CURRENT_USER,
                        k.as_ptr(),
                        v.as_ptr(),
                        RRF_RT_REG_DWORD,
                        std::ptr::null_mut(),
                        (&mut value as *mut u32).cast(),
                        &mut len,
                    );
                }
                value == 0
            }
        }
    }
    pub fn apply(self, hwnd: HWND) {
        use windows_sys::Win32::Graphics::Dwm::{
            DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
        };
        use windows_sys::Win32::UI::{Controls::MARGINS, WindowsAndMessaging::*};
        // Unsupported attributes fail harmlessly on older Windows builds.
        let palette = super::theme::Palette::system_high_contrast().unwrap_or_else(|| {
            if self.dark() {
                super::theme::Palette::dark()
            } else {
                super::theme::Palette::light()
            }
        });
        super::theme::select(palette);
        let dark = (!palette.high_contrast && self.dark()) as u32;
        let rounded = 2u32;
        let backdrop = if self.transparency && !palette.high_contrast {
            if unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32 & WS_POPUP != 0 {
                3u32
            } else {
                2u32
            }
        } else {
            1
        };
        unsafe {
            DwmSetWindowAttribute(hwnd, 20, (&dark as *const u32).cast(), 4);
            DwmSetWindowAttribute(hwnd, 33, (&rounded as *const u32).cast(), 4);
            DwmSetWindowAttribute(hwnd, 38, (&backdrop as *const u32).cast(), 4);
            let margin = 0; // GDI controls always paint onto opaque client surfaces.
            let margins = MARGINS {
                cxLeftWidth: margin,
                cxRightWidth: margin,
                cyTopHeight: margin,
                cyBottomHeight: margin,
            };
            DwmExtendFrameIntoClientArea(hwnd, &margins);
            let key: Vec<_> = "HyperSearch.Glass".encode_utf16().chain(Some(0)).collect();
            SetPropW(hwnd, key.as_ptr(), 0usize as _);
            EnumChildWindows(hwnd, Some(super::theme::apply_child), 0);
            let tip = GetPropW(
                hwnd,
                "HyperSearch.Tooltips\0"
                    .encode_utf16()
                    .collect::<Vec<_>>()
                    .as_ptr(),
            );
            if !tip.is_null() {
                super::theme::apply_child(tip, 0);
            }
            windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appearance_roundtrip() {
        let p = std::env::temp_dir().join(format!("hs-look-{}", std::process::id()));
        let a = Appearance {
            theme: Theme::Dark,
            transparency: false,
            animations: false,
            font_percent: 120,
        };
        a.save(&p).unwrap();
        let b = Appearance::load(&p);
        assert_eq!(b.theme, Theme::Dark);
        assert!(!b.transparency);
        assert_eq!(b.font_percent, 120);
        std::fs::remove_file(p).unwrap();
    }
}
