//! Opaque text surfaces above DWM. No alpha-dependent GDI text or decorative timer.
use std::{cell::Cell, ptr::null};
use windows_sys::Win32::UI::{Accessibility::*, Input::KeyboardAndMouse::IsWindowEnabled};
use windows_sys::Win32::{
    Foundation::*,
    Graphics::Gdi::*,
    UI::{
        Controls::*,
        Shell::{DefSubclassProc, SetWindowSubclass},
        WindowsAndMessaging::*,
    },
};
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub window: u32,
    pub surface: u32,
    pub glass: u32,
    pub text: u32,
    pub secondary: u32,
    pub disabled: u32,
    pub border: u32,
    pub accent: u32,
    pub hover: u32,
    pub selected: u32,
    pub warning: u32,
    pub error: u32,
    pub high_contrast: bool,
}
const fn rgb(r: u32, g: u32, b: u32) -> u32 {
    r | (g << 8) | (b << 16)
}
impl Palette {
    pub const fn light() -> Self {
        Self {
            window: rgb(215, 232, 250),
            surface: rgb(244, 248, 255),
            glass: rgb(223, 236, 252),
            text: rgb(25, 34, 48),
            secondary: rgb(76, 88, 106),
            disabled: rgb(102, 112, 125),
            border: rgb(170, 191, 219),
            accent: rgb(0, 83, 173),
            hover: rgb(223, 233, 247),
            selected: rgb(205, 225, 252),
            warning: rgb(134, 76, 0),
            error: rgb(170, 30, 45),
            high_contrast: false,
        }
    }
    pub const fn dark() -> Self {
        Self {
            window: rgb(21, 26, 35),
            surface: rgb(30, 37, 49),
            glass: rgb(35, 44, 59),
            text: rgb(241, 245, 252),
            secondary: rgb(185, 199, 220),
            disabled: rgb(153, 168, 189),
            border: rgb(94, 113, 140),
            accent: rgb(141, 192, 255),
            hover: rgb(48, 66, 91),
            selected: rgb(47, 75, 110),
            warning: rgb(255, 204, 112),
            error: rgb(255, 157, 168),
            high_contrast: false,
        }
    }
    pub fn system_high_contrast() -> Option<Self> {
        unsafe {
            let mut hc: HIGHCONTRASTW = std::mem::zeroed();
            hc.cbSize = std::mem::size_of::<HIGHCONTRASTW>() as u32;
            if SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                hc.cbSize,
                (&mut hc as *mut HIGHCONTRASTW).cast(),
                0,
            ) == 0
                || hc.dwFlags & HCF_HIGHCONTRASTON == 0
            {
                return None;
            }
            Some(Self {
                window: GetSysColor(COLOR_WINDOW),
                surface: GetSysColor(COLOR_WINDOW),
                glass: GetSysColor(COLOR_WINDOW),
                text: GetSysColor(COLOR_WINDOWTEXT),
                secondary: GetSysColor(COLOR_WINDOWTEXT),
                disabled: GetSysColor(COLOR_GRAYTEXT),
                border: GetSysColor(COLOR_WINDOWTEXT),
                accent: GetSysColor(COLOR_HIGHLIGHTTEXT),
                hover: GetSysColor(COLOR_HIGHLIGHT),
                selected: GetSysColor(COLOR_HIGHLIGHT),
                warning: GetSysColor(COLOR_WINDOWTEXT),
                error: GetSysColor(COLOR_WINDOWTEXT),
                high_contrast: true,
            })
        }
    }
}
thread_local! { static CURRENT:Cell<Palette>=const {Cell::new(Palette::light())}; }
thread_local! {static PAINTS:Cell<(u64,f64,u64)>=const{Cell::new((0,0.,0))};static TIP_TEXT:std::cell::RefCell<Vec<Vec<u16>>>=const{std::cell::RefCell::new(Vec::new())};}
pub fn paint_stats() -> (u64, f64, u64) {
    PAINTS.with(Cell::get)
}
pub fn tooltips(owner: HWND, controls: &[(HWND, &str)]) {
    unsafe {
        let tip = CreateWindowExW(
            WS_EX_TOPMOST,
            wide("tooltips_class32").as_ptr(),
            null(),
            WS_POPUP | TTS_ALWAYSTIP,
            0,
            0,
            0,
            0,
            owner,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            null(),
        );
        SendMessageW(tip, TTM_SETMAXTIPWIDTH, 0, 420);
        SetPropW(owner, wide("HyperSearch.Tooltips").as_ptr(), tip);
        for (control, label) in controls {
            TIP_TEXT.with(|texts| {
                let mut texts = texts.borrow_mut();
                texts.push(wide(label));
                let mut info: TTTOOLINFOW = std::mem::zeroed();
                info.cbSize = std::mem::size_of::<TTTOOLINFOW>() as u32;
                info.uFlags = TTF_IDISHWND | TTF_SUBCLASS;
                info.hwnd = owner;
                info.uId = *control as usize;
                info.lpszText = texts.last_mut().unwrap().as_mut_ptr();
                SendMessageW(tip, TTM_ADDTOOLW, 0, &info as *const _ as isize);
            });
        }
        apply_child(tip, 0);
    }
}
pub fn current() -> Palette {
    CURRENT.with(Cell::get)
}
pub fn select(p: Palette) {
    CURRENT.with(|c| c.set(p));
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn class(hwnd: HWND) -> String {
    unsafe {
        let mut b = [0u16; 64];
        let n = GetClassNameW(hwnd, b.as_mut_ptr(), 64);
        String::from_utf16_lossy(&b[..n as usize])
    }
}
pub unsafe extern "system" fn apply_child(hwnd: HWND, _: isize) -> i32 {
    let p = current();
    unsafe {
        match class(hwnd).as_str() {
            "SysListView32" => {
                SendMessageW(hwnd, LVM_SETBKCOLOR, 0, p.surface as isize);
                SendMessageW(hwnd, LVM_SETTEXTBKCOLOR, 0, p.surface as isize);
                SendMessageW(hwnd, LVM_SETTEXTCOLOR, 0, p.text as isize);
                SetWindowSubclass(hwnd, Some(list_proc), 1, 0);
                let header = SendMessageW(hwnd, LVM_GETHEADER, 0, 0) as HWND;
                SetWindowTheme(header, wide("").as_ptr(), wide("").as_ptr());
            }
            "SysTreeView32" => {
                SendMessageW(hwnd, TVM_SETBKCOLOR, 0, p.surface as isize);
                SendMessageW(hwnd, TVM_SETTEXTCOLOR, 0, p.text as isize);
            }
            "tooltips_class32" => {
                SendMessageW(hwnd, TTM_SETTIPBKCOLOR, p.surface as usize, 0);
                SendMessageW(hwnd, TTM_SETTIPTEXTCOLOR, p.text as usize, 0);
            }
            "ComboBox" => {
                SetWindowTheme(hwnd, wide("").as_ptr(), wide("").as_ptr());
            }
            _ => {}
        }
        InvalidateRect(hwnd, null(), 1);
    }
    1
}
unsafe extern "system" fn list_proc(
    hwnd: HWND,
    msg: u32,
    w: WPARAM,
    l: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    unsafe {
        if msg == WM_NOTIFY && l != 0 && (*(l as *const NMHDR)).code == NM_CUSTOMDRAW {
            let d = &mut *(l as *mut NMCUSTOMDRAW);
            let p = current();
            if d.dwDrawStage == CDDS_PREPAINT {
                return CDRF_NOTIFYITEMDRAW as isize;
            }
            if d.dwDrawStage == CDDS_ITEMPREPAINT {
                rounded(d.hdc, &d.rc, p.glass, p.border, 0);
                let mut text = [0u16; 256];
                let mut item: HDITEMW = std::mem::zeroed();
                item.mask = HDI_TEXT;
                item.pszText = text.as_mut_ptr();
                item.cchTextMax = 256;
                SendMessageW(
                    d.hdr.hwndFrom,
                    HDM_GETITEMW,
                    d.dwItemSpec,
                    &mut item as *mut _ as isize,
                );
                SetTextColor(d.hdc, p.secondary);
                SetBkMode(d.hdc, TRANSPARENT as i32);
                let mut r = d.rc;
                r.left += 10;
                DrawTextW(
                    d.hdc,
                    text.as_ptr(),
                    -1,
                    &mut r,
                    DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                );
                return CDRF_SKIPDEFAULT as isize;
            }
        }
        DefSubclassProc(hwnd, msg, w, l)
    }
}
fn rounded(dc: HDC, r: &RECT, bg: u32, border: u32, radius: i32) {
    unsafe {
        let brush = CreateSolidBrush(bg);
        let pen = CreatePen(PS_SOLID, 1, border);
        let oldb = SelectObject(dc, brush as _);
        let oldp = SelectObject(dc, pen as _);
        RoundRect(dc, r.left, r.top, r.right, r.bottom, radius, radius);
        SelectObject(dc, oldp);
        SelectObject(dc, oldb);
        DeleteObject(pen as _);
        DeleteObject(brush as _);
    }
}
pub fn message(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> Option<LRESULT> {
    let p = current();
    unsafe {
        match msg {
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN
            | WM_CTLCOLORDLG => {
                let dc = w as HDC;
                SetTextColor(
                    dc,
                    if IsWindowEnabled(l as HWND) == 0 {
                        p.disabled
                    } else {
                        p.text
                    },
                );
                let bg = if msg == WM_CTLCOLOREDIT
                    || msg == WM_CTLCOLORLISTBOX
                    || matches!(GetDlgCtrlID(l as HWND), 143 | 170 | 187 | 188)
                {
                    p.surface
                } else {
                    p.window
                };
                SetBkMode(dc, OPAQUE as i32);
                SetBkColor(dc, bg);
                SetDCBrushColor(dc, bg);
                Some(GetStockObject(DC_BRUSH) as isize)
            }
            WM_ERASEBKGND => {
                let mut r = std::mem::zeroed();
                GetClientRect(hwnd, &mut r);
                SetDCBrushColor(w as HDC, p.window);
                FillRect(w as HDC, &r, GetStockObject(DC_BRUSH) as _);
                Some(1)
            }
            WM_PAINT => {
                let start = std::time::Instant::now();
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                SetDCBrushColor(dc, p.window);
                FillRect(dc, &ps.rcPaint, GetStockObject(DC_BRUSH) as _);
                if !p.high_contrast {
                    let mut full: RECT = std::mem::zeroed();
                    GetClientRect(hwnd, &mut full);
                    for band in 0..32 {
                        let mut color = 0;
                        for shift in [0, 8, 16] {
                            let a = (p.window >> shift) & 255;
                            let b = (p.glass >> shift) & 255;
                            color |= ((a * (32 - band) + b * band) / 32) << shift;
                        }
                        let r = RECT {
                            left: 0,
                            right: full.right,
                            top: full.bottom * band as i32 / 32,
                            bottom: full.bottom * (band + 1) as i32 / 32,
                        };
                        SetDCBrushColor(dc, color);
                        FillRect(dc, &r, GetStockObject(DC_BRUSH) as _);
                    }
                }
                for id in [107, 115, 143, 146, 104, 116] {
                    let child = GetDlgItem(hwnd, id);
                    if child.is_null() || IsWindowVisible(child) == 0 {
                        continue;
                    }
                    let mut r = std::mem::zeroed();
                    GetWindowRect(child, &mut r);
                    MapWindowPoints(std::ptr::null_mut(), hwnd, (&mut r as *mut RECT).cast(), 2);
                    if id == 143 {
                        r.top -= 56;
                        r.bottom += 132;
                    }
                    r.left -= 5;
                    r.top -= 5;
                    r.right += 5;
                    r.bottom += 5;
                    rounded(
                        dc,
                        &r,
                        if id == 115 || id == 146 {
                            p.glass
                        } else {
                            p.surface
                        },
                        p.border,
                        22,
                    );
                }
                EndPaint(hwnd, &ps);
                PAINTS.with(|s| {
                    let (n, ms, buttons) = s.get();
                    s.set((n + 1, ms + start.elapsed().as_secs_f64() * 1000., buttons));
                });
                Some(0)
            }
            WM_NOTIFY if l != 0 && (*(l as *const NMHDR)).code == NM_CUSTOMDRAW => {
                let d = &mut *(l as *mut NMCUSTOMDRAW);
                if class(d.hdr.hwndFrom) == "Button" && d.dwDrawStage == CDDS_PREPAINT {
                    PAINTS.with(|s| {
                        let (n, ms, buttons) = s.get();
                        s.set((n, ms, buttons + 1));
                    });
                    let disabled =
                        d.uItemState & CDIS_DISABLED != 0 || IsWindowEnabled(d.hdr.hwndFrom) == 0;
                    let hot = d.uItemState & (CDIS_HOT | CDIS_SELECTED) != 0;
                    let id = GetDlgCtrlID(d.hdr.hwndFrom);
                    let active = (180..=186).contains(&id)
                        && SendMessageW(GetDlgItem(GetParent(d.hdr.hwndFrom), 152), 0x147, 0, 0)
                            == (id - 180) as isize;
                    rounded(
                        d.hdc,
                        &d.rc,
                        if active {
                            p.selected
                        } else if hot {
                            p.hover
                        } else {
                            p.surface
                        },
                        if hot { p.accent } else { p.border },
                        10,
                    );
                    SetBkMode(d.hdc, TRANSPARENT as i32);
                    SetTextColor(
                        d.hdc,
                        if disabled {
                            p.disabled
                        } else if hot && p.high_contrast {
                            p.accent
                        } else {
                            p.text
                        },
                    );
                    if GetDlgCtrlID(d.hdr.hwndFrom) == 108 && !(hot && p.high_contrast) {
                        let fg = super::status::STATUS.with(|s| match s.borrow().freshness {
                            super::status::Freshness::Unavailable => p.error,
                            super::status::Freshness::Unverified => p.warning,
                            _ => p.secondary,
                        });
                        SetTextColor(d.hdc, fg);
                    }
                    let mut b = [0u16; 512];
                    GetWindowTextW(d.hdr.hwndFrom, b.as_mut_ptr(), 512);
                    let old =
                        SelectObject(d.hdc, SendMessageW(d.hdr.hwndFrom, WM_GETFONT, 0, 0) as _);
                    let mut r = d.rc;
                    r.left += 8;
                    r.right -= 8;
                    let style = GetWindowLongW(d.hdr.hwndFrom, GWL_STYLE) as u32 & 15;
                    if style == BS_AUTOCHECKBOX as u32 || style == BS_CHECKBOX as u32 {
                        let checked =
                            SendMessageW(d.hdr.hwndFrom, BM_GETCHECK, 0, 0) == BST_CHECKED as isize;
                        let label = if checked { "[x] " } else { "[ ] " };
                        let text = label.to_owned()
                            + &String::from_utf16_lossy(
                                &b[..b.iter().position(|&c| c == 0).unwrap_or(0)],
                            );
                        let b = wide(&text);
                        DrawTextW(
                            d.hdc,
                            b.as_ptr(),
                            -1,
                            &mut r,
                            DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                        );
                    } else {
                        DrawTextW(
                            d.hdc,
                            b.as_ptr(),
                            -1,
                            &mut r,
                            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
                        );
                    }
                    if d.uItemState & CDIS_FOCUS != 0 {
                        r.left += 2;
                        r.right -= 2;
                        DrawFocusRect(d.hdc, &r);
                    }
                    SelectObject(d.hdc, old);
                    return Some(CDRF_SKIPDEFAULT as isize);
                }
                if class(d.hdr.hwndFrom) == "SysListView32" {
                    if d.dwDrawStage == CDDS_PREPAINT {
                        return Some(CDRF_NOTIFYITEMDRAW as isize);
                    }
                    if d.dwDrawStage == CDDS_ITEMPREPAINT {
                        let d = &mut *(l as *mut NMLVCUSTOMDRAW);
                        let selected = SendMessageW(
                            d.nmcd.hdr.hwndFrom,
                            LVM_GETITEMSTATE,
                            d.nmcd.dwItemSpec,
                            LVIS_SELECTED as isize,
                        ) & LVIS_SELECTED as isize
                            != 0;
                        d.clrText = if p.high_contrast && selected {
                            p.accent
                        } else {
                            p.text
                        };
                        d.clrTextBk = if selected { p.selected } else { p.surface };
                        return Some(CDRF_DODEFAULT as isize);
                    }
                }
                None
            }
            _ => None,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_control_colors_switch_and_stay_opaque() {
        unsafe {
            let dc = CreateCompatibleDC(std::ptr::null_mut());
            let control = CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide("text").as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                100,
                30,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                null(),
            );
            for p in [
                Palette::light(),
                Palette::dark(),
                Palette {
                    high_contrast: true,
                    ..Palette::dark()
                },
            ] {
                select(p);
                message(
                    std::ptr::null_mut(),
                    WM_CTLCOLOREDIT,
                    dc as usize,
                    control as isize,
                )
                .unwrap();
                assert_eq!(GetTextColor(dc), p.text);
                assert_eq!(GetBkColor(dc), p.surface);
                assert_eq!(GetBkMode(dc), OPAQUE as i32);
            }
            DestroyWindow(control);
            DeleteDC(dc);
        }
    }
    fn luminance(c: u32) -> f64 {
        let mut l = 0.;
        for (shift, weight) in [(0, 0.2126), (8, 0.7152), (16, 0.0722)] {
            let v = ((c >> shift) & 255) as f64 / 255.;
            l += weight
                * if v <= 0.04045 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                };
        }
        l
    }
    fn contrast(a: u32, b: u32) -> f64 {
        let a = luminance(a);
        let b = luminance(b);
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }
    #[test]
    fn text_surfaces_meet_aa() {
        for p in [Palette::light(), Palette::dark()] {
            for bg in [p.window, p.surface, p.glass, p.hover, p.selected] {
                assert!(contrast(p.text, bg) >= 4.5);
                assert!(contrast(p.secondary, bg) >= 4.5);
            }
            for fg in [p.disabled, p.warning, p.error] {
                assert!(contrast(fg, p.surface) >= 4.5);
            }
        }
    }
}
