//! Live DWM host backdrop. Never reads wallpaper files or captures the desktop.
use std::{cell::RefCell, collections::HashMap};
use windows::{
    Foundation::Numerics::Vector2,
    System::DispatcherQueueController,
    UI::Composition::Desktop::DesktopWindowTarget,
    UI::Composition::{Compositor, ContainerVisual, SpriteVisual},
    Win32::{
        Foundation::HWND as WinHwnd,
        System::{
            Com::{COINIT_APARTMENTTHREADED, CoInitializeEx},
            WinRT::{
                Composition::ICompositorDesktopInterop, CreateDispatcherQueueController,
                DQTAT_COM_STA, DQTYPE_THREAD_CURRENT, DispatcherQueueOptions,
            },
        },
    },
    core::Interface,
};
use windows_sys::Win32::{
    Foundation::{HWND, RECT},
    Graphics::{Dwm::*, Gdi::*},
    UI::{Controls::*, WindowsAndMessaging::*},
};
struct Window {
    target: DesktopWindowTarget,
    root: ContainerVisual,
    material: SpriteVisual,
    active: bool,
}
#[derive(Default)]
struct State {
    queue: Option<DispatcherQueueController>,
    compositor: Option<Compositor>,
    windows: HashMap<usize, Window>,
    failed: bool,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
unsafe extern "system" fn layer_child(child: HWND, enabled: isize) -> i32 {
    unsafe {
        let mut name = [0u16; 64];
        let n = GetClassNameW(child, name.as_mut_ptr(), 64);
        let name = String::from_utf16_lossy(&name[..n as usize]);
        if matches!(
            name.as_str(),
            "Button" | "Edit" | "Static" | "ListBox" | "SysListView32" | "ComboBox"
        ) {
            let style = GetWindowLongW(child, GWL_EXSTYLE) as u32;
            if enabled != 0 {
                SetWindowLongW(child, GWL_EXSTYLE, (style | WS_EX_LAYERED) as i32);
                let alpha = if name == "SysListView32" || name == "ListBox" || name == "Edit" {
                    210
                } else {
                    240
                };
                SetLayeredWindowAttributes(child, 0, alpha, LWA_ALPHA);
            } else if style & WS_EX_LAYERED != 0 {
                SetLayeredWindowAttributes(child, 0, 255, LWA_ALPHA);
                SetWindowLongW(child, GWL_EXSTYLE, (style & !WS_EX_LAYERED) as i32);
            }
            if name == "SysListView32" {
                let image: LVBKIMAGEW = std::mem::zeroed();
                SendMessageW(child, LVM_SETBKIMAGEW, 0, &image as *const _ as isize);
                RemovePropW(
                    child,
                    "HyperSearch.GlassImage\0"
                        .encode_utf16()
                        .collect::<Vec<_>>()
                        .as_ptr(),
                );
            }
        }
        1
    }
}
pub fn paint(hwnd: HWND, dc: HDC, _: u32) -> bool {
    STATE.with(|cell| {
        let Ok(mut state) = cell.try_borrow_mut() else {
            return false;
        };
        if state.failed {
            return false;
        }
        let result = (|| -> windows::core::Result<()> {
            unsafe {
                if state.compositor.is_none() {
                    let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
                    state.queue = CreateDispatcherQueueController(DispatcherQueueOptions {
                        dwSize: std::mem::size_of::<DispatcherQueueOptions>() as u32,
                        threadType: DQTYPE_THREAD_CURRENT,
                        apartmentType: DQTAT_COM_STA,
                    })
                    .ok();
                    state.compositor = Some(Compositor::new()?);
                }
                let enabled = 1u32;
                DwmSetWindowAttribute(hwnd, 17, (&enabled as *const u32).cast(), 4);
                if !state.windows.contains_key(&(hwnd as usize)) {
                    let compositor = state.compositor.as_ref().unwrap();
                    let interop: ICompositorDesktopInterop = compositor.cast()?;
                    let target = interop.CreateDesktopWindowTarget(WinHwnd(hwnd), false)?;
                    let root = compositor.CreateContainerVisual()?;
                    root.SetRelativeSizeAdjustment(Vector2 { X: 1., Y: 1. })?;
                    let material = compositor.CreateSpriteVisual()?;
                    material.SetRelativeSizeAdjustment(Vector2 { X: 1., Y: 1. })?;
                    material.SetBrush(&compositor.CreateHostBackdropBrush()?)?;
                    root.Children()?.InsertAtBottom(&material)?;
                    target.SetRoot(&root)?;
                    EnumChildWindows(hwnd, Some(layer_child), 1);
                    state.windows.insert(
                        hwnd as usize,
                        Window {
                            target,
                            root,
                            material,
                            active: true,
                        },
                    );
                }
                let window = state.windows.get_mut(&(hwnd as usize)).unwrap();
                if !window.active {
                    EnumChildWindows(hwnd, Some(layer_child), 1);
                    window.active = true;
                }
                window.root.SetOpacity(1.)?;
                let _ = &window.target;
                let _ = &window.material;
                let margins = windows_sys::Win32::UI::Controls::MARGINS {
                    cxLeftWidth: -1,
                    cxRightWidth: -1,
                    cyTopHeight: -1,
                    cyBottomHeight: -1,
                };
                DwmExtendFrameIntoClientArea(hwnd, &margins);
                let mut r: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut r);
                SetDCBrushColor(dc, 0);
                FillRect(dc, &r, GetStockObject(DC_BRUSH) as _);
                Ok(())
            }
        })();
        if let Err(error) = result {
            state.failed = true;
            let _ = std::fs::write(
                std::env::temp_dir().join("hypersearch-live-glass-error.txt"),
                error.to_string(),
            );
            return false;
        }
        true
    })
}
pub fn disable(hwnd: HWND) {
    STATE.with(|state| {
        if let Ok(mut state) = state.try_borrow_mut()
            && let Some(window) = state.windows.get_mut(&(hwnd as usize))
        {
            let _ = window.root.SetOpacity(0.);
            window.active = false;
        }
    });
    unsafe {
        EnumChildWindows(hwnd, Some(layer_child), 0);
    }
}
pub fn control_brush(_: HWND, _: HWND) -> Option<HBRUSH> {
    None
}
