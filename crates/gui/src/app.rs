use super::view_model::{self, Row};
use hypersearch_engine::{
    filesystem, indexer, monitor,
    planning::{self, Summary},
    search::{MemoryMode, Mode, SearchIndex},
    storage,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    ptr::null,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Instant,
};
use windows_sys::Win32::UI::HiDpi::*;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows_sys::Win32::UI::{Controls::*, Shell::ShellExecuteW};
use windows_sys::Win32::{
    Foundation::*, Graphics::Gdi::*, System::LibraryLoader::*, UI::WindowsAndMessaging::*,
};

const DONE: u32 = WM_APP + 1;
const ROOT: usize = 101;
const BUILD: usize = 102;
const LOAD: usize = 103;
const QUERY: usize = 104;
const MODE: usize = 105;
const NEXT: usize = 106;
const OUTPUT: usize = 107;
const STATUS: usize = 108;
const ALL: usize = 109;
const PROFILE: usize = 110;
const BUDGET: usize = 111;
const ANALYZE: usize = 112;
const APPLY: usize = 113;
const PLAN: usize = 114;
const SIDEBAR: usize = 115;
const ADDRESS: usize = 116;
const GO: usize = 117;
const BACK: usize = 118;
const UP: usize = 119;
const OPEN: usize = 120;
const LOCATION: usize = 121;
const SETTINGS: usize = 122;
const WDS: usize = 123;
const WDS_PATH: usize = 124;
const WDS_SAVE: usize = 125;
const WDS_SITE: usize = 126;
const WDS_UPDATE: usize = 127;
const WDS_LABEL: usize = 128;
const THEME: usize = 129;
const GLASS: usize = 130;
const EXPORT: usize = 131;
const AI_REPORT: usize = 132;
const CANCEL_EXPORT: usize = 133;
const EXPORT_FORMAT: usize = 134;
const SAFE_EXPORT: usize = 135;
const FORWARD: usize = 136;
const VIEW: usize = 137;
const PIN: usize = 138;
const SPOTLIGHT: usize = 139;
const HOTKEY: usize = 140;
const HOTKEY_SAVE: usize = 141;
const HOTKEY_LABEL: usize = 142;
const DETAILS: usize = 143;
const COPY: usize = 144;
const COPY_PROMPT: usize = 145;
const SECTIONS: usize = 146;
const ABOUT: usize = 147;
const SEARCH_HELP: usize = 148;
const PERF_HELP: usize = 149;
const ANIMATIONS: usize = 150;
const EXTENSION: usize = 151;
const CATEGORY: usize = 152;
const RANK: usize = 153;
const WDS_SCOPE: usize = 154;
const EXPORT_ROOT: usize = 155;
const EXPORT_DEPTH: usize = 156;
const EXPORT_MIN: usize = 157;
const EXPORT_NEUTRAL: usize = 158;
const EXPORT_EXCLUDE: usize = 159;
const EXPORT_COMPACT: usize = 160;
const EXPORT_HELP: usize = 161;
enum Task {
    Download(Arc<AtomicBool>),
    Icons(Vec<String>),
    Opened(String),
    Refresh,
    Metadata(String),
    Export {
        options: hypersearch_engine::storage_analysis::Options,
        destination: PathBuf,
        cancel: Arc<AtomicBool>,
    },
    Load(MemoryMode),
    Build(String, MemoryMode),
    Analyze(String),
    Search {
        text: String,
        mode: Mode,
        after: Option<u32>,
        generation: u64,
        scope: Option<String>,
    },
}
enum Event {
    Downloaded(Result<super::windirstat_download::Package, String>),
    Icons(super::icons::Icons),
    Freshness(String, bool),
    Metadata(String, String),
    ExportProgress(u64, u64),
    ExportDone(Box<Result<(PathBuf, hypersearch_engine::storage_analysis::Summary), String>>),
    Plan(Summary, String, bool, String),
    Ready {
        count: usize,
        root: String,
        status: String,
    },
    Error(String),
    Results {
        generation: u64,
        rows: Vec<Row>,
        next: Option<u32>,
        status: String,
    },
}
struct App {
    smoke: Option<Smoke>,
    hwnd: HWND,
    tasks: Sender<Task>,
    events: Receiver<Event>,
    generation: Arc<AtomicU64>,
    next: Option<u32>,
    ready: bool,
    rows: Vec<Row>,
    scope: Option<String>,
    history: Vec<Option<String>>,
    forward: Vec<Option<String>>,
    setup: bool,
    sidebar: Vec<Option<String>>,
    status: String,
    plan: Option<Summary>,
    choice: Option<MemoryMode>,
    saved: Option<super::settings::Preference>,
    plan_saved: bool,
    plan_root: String,
    export_cancel: Option<Arc<AtomicBool>>,
    download_cancel: Option<Arc<AtomicBool>>,
    closing: bool,
    icons: std::collections::HashMap<String, i32>,
    object_count: usize,
    last_search: String,
}
struct Smoke {
    start: Instant,
    query: Instant,
    phase: u8,
    log: Vec<String>,
    idle_cpu: f64,
}
struct CachedPage {
    text: String,
    mode: Mode,
    after: Option<u32>,
    scope: Option<String>,
    rows: Vec<Row>,
    next: Option<u32>,
}
thread_local! {static APP:RefCell<Option<App>>=const{RefCell::new(None)};}
thread_local! {static LOOK:std::cell::Cell<super::appearance::Appearance>=std::cell::Cell::new(super::appearance::Appearance::default());}
thread_local! {static FONTS:RefCell<std::collections::HashMap<u32,isize>>=RefCell::new(std::collections::HashMap::new());}
fn ui_font(dpi: u32) -> usize {
    FONTS.with(|fonts| {
        *fonts.borrow_mut().entry(dpi).or_insert_with(|| unsafe {
            CreateFontW(
                -(15 * dpi as i32 / 96),
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                DEFAULT_CHARSET as u32,
                0,
                0,
                CLEARTYPE_QUALITY as u32,
                0,
                wide("Segoe UI").as_ptr(),
            ) as isize
        }) as usize
    })
}
unsafe extern "system" fn update_font(hwnd: HWND, dpi: isize) -> i32 {
    unsafe {
        SendMessageW(hwnd, WM_SETFONT, ui_font(dpi as u32), 1);
    }
    1
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn index_path() -> PathBuf {
    std::env::var_os("HYPERSEARCH_INDEX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("HyperSearch/index.hsi")
        })
}
fn date(value: std::io::Result<std::time::SystemTime>) -> String {
    let Some(seconds) = value
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
    else {
        return "недоступно".into();
    };
    let ticks = seconds
        .saturating_add(11_644_473_600)
        .saturating_mul(10_000_000);
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn FileTimeToSystemTime(file: *const FILETIME, system: *mut SYSTEMTIME) -> i32;
    }
    let mut system: SYSTEMTIME = unsafe { std::mem::zeroed() };
    if unsafe { FileTimeToSystemTime(&file, &mut system) } == 0 {
        return "недоступно".into();
    }
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        system.wYear, system.wMonth, system.wDay, system.wHour, system.wMinute, system.wSecond
    )
}
fn worker(rx: Receiver<Task>, tx: Sender<Event>, generation: Arc<AtomicU64>, window: usize) {
    let (icons_tx, icons_rx) = mpsc::channel::<Vec<String>>();
    let icons_events = tx.clone();
    std::thread::spawn(move || {
        for mut keys in &icons_rx {
            while let Ok(newer) = icons_rx.try_recv() {
                keys = newer;
            }
            let icons = super::icons::load(keys);
            if icons_events.send(Event::Icons(icons)).is_err() {
                break;
            }
            unsafe {
                PostMessageW(window as HWND, DONE, 0, 0);
            }
        }
    });
    let mut history =
        hypersearch_engine::query_tools::OpenHistory::load(&index_path().with_extension("opens"));
    let (metadata_tx, metadata_rx) = mpsc::channel::<String>();
    let metadata_events = tx.clone();
    std::thread::spawn(move || {
        for mut path in &metadata_rx {
            while let Ok(newer) = metadata_rx.try_recv() {
                path = newer;
            }
            let result = match std::fs::symlink_metadata(&path) {
                Ok(m) => {
                    use std::os::windows::fs::MetadataExt;
                    let size = if m.is_dir() {
                        "Папка: размер вложений — через анализ хранилища".into()
                    } else {
                        format!("Логический размер: {} байт", m.len())
                    };
                    format!(
                        "{}\r\n{}\r\nИзменён: {}\r\nСоздан: {}\r\nАтрибуты: 0x{:X}\r\nТолько чтение: {}\r\nФизический размер: не измерен",
                        path,
                        size,
                        date(m.modified()),
                        date(m.created()),
                        m.file_attributes(),
                        m.permissions().readonly()
                    )
                }
                Err(e) => format!("{}\r\nМетаданные недоступны: {e}", path),
            };
            if metadata_events.send(Event::Metadata(path, result)).is_err() {
                break;
            }
            unsafe {
                PostMessageW(window as HWND, DONE, 0, 0);
            }
        }
    });
    let mut index: Option<Arc<SearchIndex>> = None;
    let mut monitor_enabled = true;
    // One most recent page is enough to make immediate repeated queries allocation-light.
    let mut cache: Option<CachedPage> = None;
    for task in rx {
        let event = match task {
            Task::Download(cancel) => {
                let tx = tx.clone();
                std::thread::spawn(move || {
                    let result =
                        super::windirstat_download::download(&cancel).map_err(|e| e.to_string());
                    let _ = tx.send(Event::Downloaded(result));
                    unsafe {
                        PostMessageW(window as HWND, DONE, 0, 0);
                    }
                });
                continue;
            }
            Task::Icons(keys) => {
                let _ = icons_tx.send(keys);
                continue;
            }
            Task::Opened(path) => {
                let _ = history.record(path, &index_path().with_extension("opens"));
                cache = None;
                continue;
            }
            Task::Refresh => {
                if !monitor_enabled {
                    continue;
                }
                let Some(current) = index.as_mut().and_then(Arc::get_mut) else {
                    continue;
                };
                match monitor::refresh(current) {
                    Ok(0) => continue,
                    Ok(count) => {
                        cache = None;
                        Event::Freshness(
                            format!(
                                "Фон USN: применено {count} записей без перестроения поисковых таблиц"
                            ),
                            true,
                        )
                    }
                    Err(e) => {
                        monitor_enabled = false;
                        cache = None;
                        Event::Freshness(
                            format!(
                                "Актуальность не подтверждена: {e}. Сохранённый индекс доступен."
                            ),
                            false,
                        )
                    }
                }
            }
            Task::Metadata(path) => {
                let _ = metadata_tx.send(path);
                continue;
            }
            Task::Export {
                options,
                destination,
                cancel,
            } => {
                if let Some(index) = index.clone() {
                    let tx = tx.clone();
                    std::thread::spawn(move || {
                        let mut last = Instant::now();
                        let result = hypersearch_engine::storage_analysis::export(
                            &index.volumes,
                            &options,
                            &destination,
                            &cancel,
                            |n, total| {
                                if last.elapsed().as_millis() >= 100 {
                                    let _ = tx.send(Event::ExportProgress(n, total));
                                    unsafe {
                                        PostMessageW(window as HWND, DONE, 0, 0);
                                    }
                                    last = Instant::now();
                                }
                            },
                        )
                        .map(|s| (destination, s))
                        .map_err(|e| e.to_string());
                        let _ = tx.send(Event::ExportDone(Box::new(result)));
                        unsafe {
                            PostMessageW(window as HWND, DONE, 0, 0);
                        }
                    });
                    continue;
                } else {
                    Event::ExportDone(Box::new(Err("Сначала загрузите поисковый индекс".into())))
                }
            }
            Task::Analyze(root) => {
                index = None;
                cache = None;
                let roots = if root.trim() == "*" {
                    filesystem::drives()
                } else {
                    vec![root.trim().to_string()]
                };
                let saved = root.trim() == "*" && index_path().is_file();
                let summary = if saved {
                    planning::analyze_index(&index_path())
                } else {
                    planning::analyze_roots(&roots)
                };
                match summary {
                    Ok(s) => Event::Plan(
                        s,
                        if saved {
                            "По сохранённому индексу; новые файлы могут изменить оценку.".into()
                        } else {
                            "По доступным каталогам; закрытые объекты не включены.".into()
                        },
                        saved,
                        root,
                    ),
                    Err(e) => Event::Error(format!("Не удалось оценить память: {e}")),
                }
            }
            Task::Load(memory) | Task::Build(_, memory) => {
                monitor_enabled = true;
                drop(index.take());
                cache = None;
                let start = Instant::now();
                let mut replay_unavailable = false;
                let loaded = (|| -> Result<_, String> {
                    let mut volumes=match task {
                        Task::Build(root,_)=>{
                            let roots=if root.trim()=="*"{filesystem::drives()}else{vec![root.trim().to_string()]};
                            if roots.is_empty(){return Err("Нет доступных дисков".into());}
                            roots.iter().map(|r|indexer::index(r,false).map_err(|e|format!("{r}: {e}"))).collect::<Result<Vec<_>,_>>()?
                        },
                        _=>storage::load(&index_path()).map_err(|e|format!("Не удалось загрузить индекс: {e}. Нажмите «Все диски» или укажите каталог."))?
                    };
                    for v in &mut volumes {
                        if v.journal != 0 {
                            // Access denied does not prove a journal gap. Keep the persisted
                            // snapshot searchable, but explicitly mark its unverified freshness.
                            if let Err(e) = monitor::catch_up(v) {
                                if e.kind() == std::io::ErrorKind::PermissionDenied {
                                    replay_unavailable = true;
                                } else {
                                    return Err(format!("Индекс требует перестроения: {e}"));
                                }
                            }
                        }
                    }
                    hypersearch_engine::state_files::exclude(&mut volumes, &index_path());
                    storage::save(&index_path(), &volumes).map_err(|e| e.to_string())?;
                    Ok(volumes)
                })();
                match loaded {
                    Ok(volumes) => {
                        let root = if volumes.len() > 1 {
                            "*".into()
                        } else {
                            volumes.first().map(|v| v.root.clone()).unwrap_or_default()
                        };
                        let scope = volumes
                            .iter()
                            .map(|v| v.root.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        let snapshot = replay_unavailable || volumes.iter().any(|v| v.journal == 0);
                        let warnings: usize = volumes.iter().map(|v| v.warnings.len()).sum();
                        let (built, table_cache) = SearchIndex::with_disk_cache(
                            volumes,
                            memory,
                            &hypersearch_engine::search::cache_path(&index_path(), memory),
                        );
                        let cache_detail = table_cache.detail;
                        let count = built.len();
                        let allocation = built.allocation_bytes().div_ceil(1024 * 1024);
                        index = Some(Arc::new(built));
                        Event::Ready {
                            count,
                            root,
                            status: format!(
                                "Область: {scope} · {count} объектов · режим {} · данные ~{allocation} МБ · готово за {:.2} с · {} · предупреждений: {warnings}\r\n{cache_detail}",
                                memory.key(),
                                start.elapsed().as_secs_f64(),
                                if snapshot {
                                    if replay_unavailable {
                                        "снимок; для проверки USN нужны права администратора"
                                    } else {
                                        "снимок, обновление вручную"
                                    }
                                } else {
                                    "USN обработан при загрузке"
                                }
                            ),
                        }
                    }
                    Err(e) => {
                        index = None;
                        Event::Error(e)
                    }
                }
            }
            Task::Search {
                text,
                mode,
                after,
                generation: version,
                scope,
            } => {
                if generation.load(Ordering::Relaxed) != version {
                    continue;
                }
                let Some(index) = &index else {
                    continue;
                };
                let start = Instant::now();
                if let Some(cached) = &cache
                    && cached.text == text
                    && cached.mode == mode
                    && cached.after == after
                    && cached.scope == scope
                {
                    let _ = tx.send(Event::Results {
                        generation: version,
                        rows: cached.rows.clone(),
                        next: cached.next,
                        status: "Повторный запрос · результат из кэша".into(),
                    });
                    unsafe {
                        PostMessageW(window as HWND, DONE, 0, 0);
                    }
                    continue;
                }
                if text.trim().is_empty() && scope.as_deref() == Some("hypersearch:recent") {
                    let rows = history
                        .1
                        .iter()
                        .take(100)
                        .map(|path| Row {
                            name: PathBuf::from(path)
                                .file_name()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default(),
                            path: path.clone(),
                            kind: "Недавний файл".into(),
                            directory: false,
                        })
                        .collect();
                    let _ = tx.send(Event::Results {
                        generation: version,
                        rows,
                        next: None,
                        status: "Недавно открытые в HyperSearch файлы; последнее открытие сверху"
                            .into(),
                    });
                    unsafe {
                        PostMessageW(window as HWND, DONE, 0, 0);
                    }
                    continue;
                }
                if text.trim().is_empty() && scope.is_none() {
                    let rows = index
                        .volumes
                        .iter()
                        .map(|v| Row {
                            name: format!("Локальный диск ({})", v.root.trim_end_matches('\\')),
                            path: v.root.clone(),
                            kind: "Диск".into(),
                            directory: true,
                        })
                        .collect();
                    let _ = tx.send(Event::Results {
                        generation: version,
                        rows,
                        next: None,
                        status: "Этот компьютер — доступные диски в индексе".into(),
                    });
                    unsafe {
                        PostMessageW(window as HWND, DONE, 0, 0);
                    }
                    continue;
                }
                let filters = hypersearch_engine::query_tools::Filters::parse(&text);
                let mut page = if text.trim().is_empty() {
                    let Some(page) = index.children(scope.as_deref().unwrap(), after, 100) else {
                        let _=tx.send(Event::Results{generation:version,rows:vec![],next:None,status:"Этой папки нет в загруженном индексе. Измените область или выполните индексацию.".into()});
                        unsafe {
                            PostMessageW(window as HWND, DONE, 0, 0);
                        }
                        continue;
                    };
                    page
                } else {
                    hypersearch_engine::query_tools::search(
                        index,
                        &filters,
                        mode,
                        scope.as_deref(),
                        after,
                        100,
                        || generation.load(Ordering::Relaxed) != version,
                    )
                };
                if filters.ranked {
                    hypersearch_engine::query_tools::rank(
                        index,
                        &mut page.ids,
                        &filters.text,
                        &history.0,
                    );
                }
                let elapsed = start.elapsed().as_secs_f64() * 1000.;
                if generation.load(Ordering::Relaxed) != version {
                    continue;
                }
                let rows = view_model::rows(index, &page.ids);
                cache = Some(CachedPage {
                    text,
                    mode,
                    after,
                    scope,
                    rows: rows.clone(),
                    next: page.next,
                });
                Event::Results {
                    generation: version,
                    rows,
                    next: page.next,
                    status: format!(
                        "Страница: {} результатов · поиск: {elapsed:.3} мс · с путями: {:.3} мс · кандидатов: {}",
                        page.ids.len(),
                        start.elapsed().as_secs_f64() * 1000.,
                        page.examined
                    ),
                }
            }
        };
        if tx.send(event).is_err() {
            break;
        }
        // Wake the UI immediately; no polling timer or intentional typing delay.
        unsafe {
            PostMessageW(window as HWND, DONE, 0, 0);
        }
    }
}
fn control(hwnd: HWND, id: usize) -> HWND {
    unsafe {
        let child = GetDlgItem(hwnd, id as i32);
        if !child.is_null() {
            return child;
        }
        GetDlgItem(
            GetPropW(hwnd, wide("HyperSearch.Settings").as_ptr()),
            id as i32,
        )
    }
}
fn palette(hwnd: HWND) -> HWND {
    unsafe { GetPropW(hwnd, wide("HyperSearch.Palette").as_ptr()) }
}
fn palette_bounds(work: &RECT, dpi: u32) -> RECT {
    let dpi = dpi.max(96) as i32;
    let width = (760 * dpi / 96).min(work.right - work.left);
    let height = (430 * dpi / 96).min(work.bottom - work.top);
    let left = work.left + (work.right - work.left - width) / 2;
    let top = work.top + (work.bottom - work.top - height) / 3;
    RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}
fn copy_text(hwnd: HWND, value: &str) -> Result<(), String> {
    use windows_sys::Win32::System::{DataExchange::*, Memory::*};
    let value = wide(value);
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, value.len() * 2);
        if memory.is_null() {
            return Err("Недостаточно памяти для буфера обмена".into());
        }
        let ptr = GlobalLock(memory) as *mut u16;
        if ptr.is_null() {
            GlobalFree(memory);
            return Err("Буфер недоступен".into());
        }
        std::ptr::copy_nonoverlapping(value.as_ptr(), ptr, value.len());
        GlobalUnlock(memory);
        if OpenClipboard(hwnd) == 0 {
            GlobalFree(memory);
            return Err("Буфер обмена занят".into());
        }
        let success = EmptyClipboard() != 0 && !SetClipboardData(13, memory).is_null();
        CloseClipboard();
        if !success {
            GlobalFree(memory);
            return Err("Не удалось скопировать".into());
        }
    }
    Ok(())
}
fn show_palette(hwnd: HWND) {
    unsafe {
        let p = palette(hwnd);
        let active = GetForegroundWindow();
        let monitor = MonitorFromWindow(
            if active.is_null() { hwnd } else { active },
            MONITOR_DEFAULTTONEAREST,
        );
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        // Move the hidden palette onto the target monitor first, so its effective
        // DPI has changed before calculating the final centered rectangle.
        SetWindowPos(
            p,
            std::ptr::null_mut(),
            info.rcWork.left,
            info.rcWork.top,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        let bounds = palette_bounds(&info.rcWork, GetDpiForWindow(p));
        SetWindowPos(
            p,
            HWND_TOP,
            bounds.left,
            bounds.top,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            SWP_NOACTIVATE,
        );
        let animated = LOOK.with(|look| look.get().animations);
        if animated && IsWindowVisible(p) == 0 {
            AnimateWindow(p, 90, AW_BLEND);
        } else {
            ShowWindow(p, SW_SHOW);
        }
        SetForegroundWindow(p);
        set(GetDlgItem(p, QUERY as i32), &text(control(hwnd, QUERY)));
        SetFocus(GetDlgItem(p, QUERY as i32));
    }
}
fn register_shortcut(hwnd: HWND, value: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    if value.trim().is_empty() || value.eq_ignore_ascii_case("off") {
        unsafe {
            UnregisterHotKey(hwnd, 1);
            RemovePropW(hwnd, wide("HyperSearch.Hotkey.Key").as_ptr());
            RemovePropW(hwnd, wide("HyperSearch.Hotkey.Modifiers").as_ptr());
        }
        return Ok(());
    }
    let mut modifiers = MOD_NOREPEAT;
    let mut key = None;
    for part in value.to_lowercase().split('+') {
        match part.trim() {
            "ctrl" => modifiers |= MOD_CONTROL,
            "alt" => modifiers |= MOD_ALT,
            "shift" => modifiers |= MOD_SHIFT,
            "win" => modifiers |= MOD_WIN,
            "space" => key = Some(0x20),
            s if s.len() == 1 && s.as_bytes()[0].is_ascii_alphanumeric() => {
                key = Some(s.as_bytes()[0].to_ascii_uppercase() as u32)
            }
            _ => return Err("Формат: Ctrl+Alt+Space, Ctrl+Shift+F или off".into()),
        }
    }
    let key = key.ok_or("Не указана клавиша")?;
    if modifiers == MOD_NOREPEAT {
        return Err("Добавьте Ctrl, Alt, Shift или Win".into());
    }
    let previous_key =
        unsafe { GetPropW(hwnd, wide("HyperSearch.Hotkey.Key").as_ptr()) } as usize as u32;
    let previous_modifiers =
        unsafe { GetPropW(hwnd, wide("HyperSearch.Hotkey.Modifiers").as_ptr()) } as usize as u32;
    unsafe {
        UnregisterHotKey(hwnd, 1);
    }
    if unsafe { RegisterHotKey(hwnd, 1, modifiers, key) } == 0 {
        if previous_key != 0 {
            unsafe {
                RegisterHotKey(hwnd, 1, previous_modifiers, previous_key);
            }
        }
        return Err("Сочетание занято системой или другой программой".into());
    }
    unsafe {
        SetPropW(
            hwnd,
            wide("HyperSearch.Hotkey.Key").as_ptr(),
            key as usize as _,
        );
        SetPropW(
            hwnd,
            wide("HyperSearch.Hotkey.Modifiers").as_ptr(),
            modifiers as usize as _,
        );
    }
    Ok(())
}
fn set(hwnd: HWND, text: &str) {
    unsafe {
        SetWindowTextW(hwnd, wide(text).as_ptr());
    }
}
fn text(hwnd: HWND) -> String {
    unsafe {
        let n = GetWindowTextLengthW(hwnd);
        let mut b = vec![0; n as usize + 1];
        let n = GetWindowTextW(hwnd, b.as_mut_ptr(), b.len() as i32);
        String::from_utf16_lossy(&b[..n as usize])
    }
}
impl App {
    fn refresh_performance(&self) {
        let metrics = hypersearch_engine::metrics::process();
        let snapshot = std::fs::metadata(index_path())
            .map(|m| m.len())
            .unwrap_or(0);
        let mode = self.choice.unwrap_or(MemoryMode::Fast);
        let cache = std::fs::metadata(hypersearch_engine::search::cache_path(&index_path(), mode))
            .map(|m| m.len())
            .unwrap_or(0);
        let ram = metrics
            .map(|m| {
                format!(
                    "Private: {:.1} MiB; Working set: {:.1} MiB",
                    m.private_bytes as f64 / 1048576.,
                    m.working_set_bytes as f64 / 1048576.
                )
            })
            .unwrap_or_else(|| "недоступно".into());
        set(
            control(self.hwnd, PERF_HELP),
            &format!(
                "RAM процесса: {ram}\r\nОбъектов при загрузке: {}\r\nСнимок на диске: {snapshot} байт\r\nКэш {}: {cache} байт\r\nПоследний поиск: {}\r\n\r\nUSN проверяется каждые 3 секунды. При отказе доступа проверка прекращается до обновления. Декоративного цикла перерисовки и захвата экрана нет.",
                self.object_count,
                mode.key(),
                self.last_search
            ),
        );
    }
    fn finish_smoke(&mut self, success: bool, detail: &str) {
        if let Some(smoke) = &mut self.smoke {
            if success
                && smoke.phase == 2
                && let Some(ms) = std::env::var("HYPERSEARCH_UI_IDLE_MS")
                    .ok()
                    .and_then(|s| s.parse::<u32>().ok())
                    .filter(|ms| *ms >= 1000 && *ms <= 60000)
            {
                smoke.log.push(format!("{detail}; starting_idle_ms={ms}"));
                smoke.phase = 4;
                smoke.query = Instant::now();
                smoke.idle_cpu = hypersearch_engine::metrics::process()
                    .map(|m| m.cpu_seconds)
                    .unwrap_or(0.);
                unsafe {
                    SetTimer(self.hwnd, 3, ms, None);
                }
                return;
            }
            smoke.log.push(format!(
                "success={success}; {detail}; total_ms={:.3}",
                smoke.start.elapsed().as_secs_f64() * 1000.
            ));
            let output = std::env::var_os("HYPERSEARCH_UI_REPORT")
                .map(PathBuf::from)
                .unwrap_or_else(|| index_path().with_extension("ui-test.txt"));
            let saved = std::fs::write(output, smoke.log.join("\n")).is_ok();
            unsafe {
                PostQuitMessage(if success && saved { 0 } else { 1 });
            }
            smoke.phase = 3;
        }
    }
    fn inspect_smoke(&mut self) {
        let Some(smoke) = &mut self.smoke else {
            return;
        };
        smoke.log.push(format!(
            "query_message_to_model_ms={:.3}; rows={}",
            smoke.query.elapsed().as_secs_f64() * 1000.,
            self.rows.len()
        ));
        let settings = unsafe { GetPropW(self.hwnd, wide("HyperSearch.Settings").as_ptr()) };
        let checks = unsafe {
            [
                (
                    SendMessageW(control(self.hwnd, SECTIONS), 0x18b, 0, 0) == 7,
                    "seven settings sections",
                ),
                (
                    GetParent(control(self.hwnd, WDS)) == settings,
                    "external tool settings owner",
                ),
                (
                    GetParent(control(self.hwnd, EXPORT_DEPTH)) == settings,
                    "export settings owner",
                ),
                (
                    GetWindowLongW(control(self.hwnd, OUTPUT), GWL_STYLE) as u32 & LVS_OWNERDATA
                        != 0,
                    "virtual main list",
                ),
                (
                    GetWindowLongW(control(self.hwnd, OUTPUT), GWL_STYLE) as u32 & LVS_SINGLESEL
                        == 0,
                    "multi-selection",
                ),
                (!palette(self.hwnd).is_null(), "separate Spotlight window"),
                (
                    SendMessageW(control(self.hwnd, OUTPUT), LVM_GETITEMCOUNT, 0, 0)
                        == self.rows.len() as isize,
                    "virtual result count",
                ),
            ]
        };
        for (passed, label) in checks {
            if !passed {
                self.finish_smoke(false, label);
                return;
            }
        }
        for view in [LV_VIEW_DETAILS, LV_VIEW_LIST, LV_VIEW_ICON] {
            let list = control(self.hwnd, OUTPUT);
            unsafe {
                SendMessageW(list, LVM_SETVIEW, view as usize, 0);
            }
            if unsafe { SendMessageW(list, LVM_GETVIEW, 0, 0) } as u32 != view {
                self.finish_smoke(false, "native view mode switch failed");
                return;
            }
        }
        unsafe {
            SendMessageW(
                control(self.hwnd, OUTPUT),
                LVM_SETVIEW,
                LV_VIEW_DETAILS as usize,
                0,
            );
        }
        let sections = [
            (0, THEME),
            (1, SEARCH_HELP),
            (2, PLAN),
            (3, EXPORT),
            (4, HOTKEY),
            (5, PERF_HELP),
            (6, ABOUT),
        ];
        for &(page, expected) in &sections {
            unsafe {
                SendMessageW(control(self.hwnd, SECTIONS), 0x186, page, 0);
            }
            layout(self.hwnd, true);
            for &(_, id) in &sections {
                let visible = unsafe {
                    GetWindowLongW(control(self.hwnd, id), GWL_STYLE) as u32 & WS_VISIBLE != 0
                };
                if visible != (id == expected) {
                    self.finish_smoke(false, "settings section visibility mismatch");
                    return;
                }
            }
        }
        layout(self.hwnd, false);
        let look = LOOK.with(|look| look.get());
        for theme in [
            super::appearance::Theme::Light,
            super::appearance::Theme::Dark,
            super::appearance::Theme::System,
        ] {
            let mut test = look;
            test.theme = theme;
            test.apply(self.hwnd);
            test.apply(settings);
            test.apply(palette(self.hwnd));
        }
        look.apply(self.hwnd);
        look.apply(settings);
        look.apply(palette(self.hwnd));
        if let Some(smoke) = &mut self.smoke {
            smoke.log.push(format!("native_controls=passed; themes_applied=3; dpi={}; monitors={}; windows_visible=false; screen_capture=false",unsafe { GetDpiForWindow(self.hwnd) },unsafe { GetSystemMetrics(SM_CMONITORS) }));
            smoke.phase = 2;
        }
        if let Some(row) = self.rows.first() {
            let _ = self.tasks.send(Task::Metadata(row.path.clone()));
        } else {
            self.finish_smoke(false, "fixture query returned no rows");
        }
    }
    fn export(&mut self, compact: bool) {
        use hypersearch_engine::storage_analysis::{self, Format, Options};
        if !self.ready || self.export_cancel.is_some() {
            return;
        }
        let mut source = self
            .scope
            .clone()
            .unwrap_or_else(|| text(control(self.hwnd, ROOT)));
        let chosen = text(control(self.hwnd, EXPORT_ROOT));
        if !chosen.trim().is_empty() {
            source = chosen;
        }
        if source.trim() == "*" || source.trim().is_empty() {
            set(
                control(self.hwnd, STATUS),
                "Выберите диск или папку для анализа.",
            );
            return;
        }
        let selected = unsafe { SendMessageW(control(self.hwnd, EXPORT_FORMAT), 0x147, 0, 0) };
        let format = if compact {
            Format::Markdown
        } else {
            match selected {
                1 => Format::Json,
                2 => Format::Csv,
                _ => Format::Markdown,
            }
        };
        let mut options = Options::safe(PathBuf::from(source), format);
        options.compact =
            compact || unsafe { SendMessageW(control(self.hwnd, EXPORT_COMPACT), 0xf0, 0, 0) } == 1;
        options.safe = unsafe { SendMessageW(control(self.hwnd, SAFE_EXPORT), 0xf0, 0, 0) } != 0;
        options.neutral_names =
            unsafe { SendMessageW(control(self.hwnd, EXPORT_NEUTRAL), 0xf0, 0, 0) } == 1;
        let parsed = text(control(self.hwnd, EXPORT_DEPTH))
            .parse::<usize>()
            .ok()
            .zip(text(control(self.hwnd, EXPORT_MIN)).parse::<u64>().ok());
        let Some((depth, min)) = parsed else {
            set(
                control(self.hwnd, STATUS),
                "Глубина и минимальный размер должны быть неотрицательными целыми числами.",
            );
            return;
        };
        options.max_depth = depth;
        options.min_bytes = min;
        options.exclusions = text(control(self.hwnd, EXPORT_EXCLUDE))
            .split(';')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
            .collect();
        let preview = format!(
            "{}\n\nПродолжить и выбрать файл отчёта?",
            storage_analysis::preview(&options)
        );
        if unsafe {
            MessageBoxW(
                self.hwnd,
                wide(&preview).as_ptr(),
                wide("Предварительный просмотр экспорта").as_ptr(),
                MB_YESNO | MB_ICONINFORMATION,
            )
        } != IDYES
        {
            return;
        }
        let extension = match format {
            Format::Json => "json",
            Format::Csv => "csv",
            Format::Markdown => "md",
        };
        let mut buffer = vec![0u16; 32768];
        let default = wide(&format!("HyperSearch-storage-report.{extension}"));
        buffer[..default.len()].copy_from_slice(&default);
        let filter = wide("Reports\0*.md;*.json;*.csv\0All files\0*.*\0");
        let ext = wide(extension);
        let mut dialog = windows_sys::Win32::UI::Controls::Dialogs::OPENFILENAMEW {
            lStructSize: std::mem::size_of::<windows_sys::Win32::UI::Controls::Dialogs::OPENFILENAMEW>(
            ) as u32,
            hwndOwner: self.hwnd,
            lpstrFile: buffer.as_mut_ptr(),
            nMaxFile: buffer.len() as u32,
            lpstrFilter: filter.as_ptr(),
            lpstrDefExt: ext.as_ptr(),
            Flags: windows_sys::Win32::UI::Controls::Dialogs::OFN_PATHMUSTEXIST
                | windows_sys::Win32::UI::Controls::Dialogs::OFN_NOCHANGEDIR,
            ..unsafe { std::mem::zeroed() }
        };
        if unsafe { windows_sys::Win32::UI::Controls::Dialogs::GetSaveFileNameW(&mut dialog) } == 0
        {
            return;
        }
        let destination = PathBuf::from(String::from_utf16_lossy(
            &buffer[..buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len())],
        ));
        let cancel = Arc::new(AtomicBool::new(false));
        self.export_cancel = Some(cancel.clone());
        let _ = self.tasks.send(Task::Export {
            options,
            destination,
            cancel,
        });
        set(
            control(self.hwnd, STATUS),
            "Анализ метаданных… Поиск остаётся доступным. Отчёт никуда не отправляется.",
        );
    }
    fn refresh_windirstat(&self) {
        if self.download_cancel.is_some() {
            set(control(self.hwnd, WDS), "Отменить загрузку");
            return;
        }
        if let Some(path) = super::windirstat::installed() {
            set(control(self.hwnd, WDS), "Открыть WinDirStat");
            set(control(self.hwnd, WDS_PATH), &path.to_string_lossy());
            set(
                control(self.hwnd, WDS_LABEL),
                "Хранилище → Анализ диска: WinDirStat найден. Путь сохранён.",
            );
        } else if super::windirstat_download::available() {
            set(control(self.hwnd, WDS), "Проверить и установить");
            set(
                control(self.hwnd, WDS_LABEL),
                "WinDirStat уже скачан. Повторная проверка SHA-256; перед установкой будет подтверждение.",
            );
        } else if let Some(path) = super::windirstat::downloaded() {
            set(
                control(self.hwnd, WDS),
                if path
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
                {
                    "Открыть архив"
                } else {
                    "Папка установщика"
                },
            );
            set(
                control(self.hwnd, WDS_LABEL),
                "Загрузка WinDirStat найдена и запомнена. Подлинность не проверена; установщик автоматически не запускается.",
            );
            set(control(self.hwnd, WDS_PATH), &path.to_string_lossy());
        } else {
            set(control(self.hwnd, WDS), "Скачать WinDirStat");
            set(
                control(self.hwnd, WDS_LABEL),
                "Хранилище → Анализ диска: WinDirStat не найден. Укажите путь или откройте официальный релиз.",
            );
        }
    }
    fn render_rows(&self) {
        unsafe {
            let p = palette(self.hwnd);
            if !p.is_null() {
                let list = GetDlgItem(p, OUTPUT as i32);
                SendMessageW(list, LVM_SETITEMCOUNT, self.rows.len(), 0);
                if IsWindowVisible(p) != 0
                    && !self.rows.is_empty()
                    && SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) < 0
                {
                    let state = LVITEMW {
                        stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                        state: LVIS_SELECTED | LVIS_FOCUSED,
                        ..std::mem::zeroed()
                    };
                    SendMessageW(list, LVM_SETITEMSTATE, 0, &state as *const _ as isize);
                }
                InvalidateRect(p, null(), 0);
            }
            let list = control(self.hwnd, OUTPUT);
            if GetWindowLongW(list, GWL_STYLE) as u32 & LVS_OWNERDATA != 0 {
                SendMessageW(list, LVM_SETITEMCOUNT, self.rows.len(), 0);
                InvalidateRect(list, null(), 0);
                return;
            }
            SendMessageW(list, WM_SETREDRAW, 0, 0);
            SendMessageW(list, LVM_DELETEALLITEMS, 0, 0);
            for (i, row) in self.rows.iter().enumerate() {
                for (column, value) in [&row.name, &row.path, &row.kind].into_iter().enumerate() {
                    let mut value = wide(value);
                    let mut item: LVITEMW = std::mem::zeroed();
                    item.mask = LVIF_TEXT;
                    item.iItem = i as i32;
                    item.iSubItem = column as i32;
                    item.pszText = value.as_mut_ptr();
                    SendMessageW(
                        list,
                        if column == 0 {
                            LVM_INSERTITEMW
                        } else {
                            LVM_SETITEMW
                        },
                        0,
                        &item as *const _ as isize,
                    );
                }
            }
            SendMessageW(list, WM_SETREDRAW, 1, 0);
            InvalidateRect(list, null(), 1);
        }
    }
    fn sort_loaded(&mut self) {
        let list = control(self.hwnd, OUTPUT);
        let column = unsafe { GetPropW(list, wide("HyperSearch.SortColumn").as_ptr()) } as usize;
        if column == 0 {
            return;
        }
        let descending =
            unsafe { GetPropW(list, wide("HyperSearch.SortDescending").as_ptr()) } as usize != 0;
        self.rows.sort_by_cached_key(|r| match column {
            2 => r.path.to_lowercase(),
            3 => r.kind.to_lowercase(),
            _ => r.name.to_lowercase(),
        });
        if descending {
            self.rows.reverse();
        }
    }
    fn selected(&self) -> Option<Row> {
        let list = unsafe {
            if IsWindowVisible(palette(self.hwnd)) != 0 {
                GetDlgItem(palette(self.hwnd), OUTPUT as i32)
            } else {
                control(self.hwnd, OUTPUT)
            }
        };
        let i = unsafe { SendMessageW(list, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) };
        if i < 0 {
            None
        } else {
            self.rows.get(i as usize).cloned()
        }
    }
    fn navigate(&mut self, path: Option<String>, remember: bool) {
        if !self.ready {
            return;
        }
        if remember {
            self.history.push(self.scope.clone());
            self.forward.clear();
        }
        self.scope = path;
        set(
            control(self.hwnd, ADDRESS),
            self.scope.as_deref().unwrap_or("Этот компьютер"),
        );
        set(control(self.hwnd, QUERY), "");
        self.search(None);
    }
    fn open(&mut self, location: bool) {
        let Some(row) = self.selected() else {
            return;
        };
        if location {
            let target = if row.directory {
                Some(row.path)
            } else {
                view_model::parent(&row.path)
            };
            if let Some(path) = target {
                self.navigate(Some(path), true);
            }
            return;
        }
        if row.directory {
            self.navigate(Some(row.path), true);
        } else {
            let result = unsafe {
                ShellExecuteW(
                    self.hwnd,
                    wide("open").as_ptr(),
                    wide(&row.path).as_ptr(),
                    null(),
                    null(),
                    SW_SHOWNORMAL,
                )
            };
            if result as isize <= 32 {
                set(
                    control(self.hwnd, STATUS),
                    "Не удалось открыть файл. Возможно, снимок индекса устарел.",
                );
            } else {
                let _ = self.tasks.send(Task::Opened(row.path));
            }
        }
    }
    fn preference(&self) -> Result<super::settings::Preference, String> {
        let Some(plan) = &self.plan else {
            return Err("Сначала нажмите «Оценить RAM».".into());
        };
        let selected = unsafe { SendMessageW(control(self.hwnd, PROFILE), 0x147, 0, 0) };
        let (mode, budget_mb) = match selected {
            1 => (MemoryMode::Balanced, None),
            2 => (MemoryMode::Fast, None),
            3 => {
                let mb: usize = text(control(self.hwnd, BUDGET))
                    .trim()
                    .parse()
                    .map_err(|_| "Введите бюджет в МБ, например 384.".to_string())?;
                (plan.budget_mode(mb).map_err(|e| e.to_string())?, Some(mb))
            }
            _ => (MemoryMode::Economy, None),
        };
        Ok(super::settings::Preference { mode, budget_mb })
    }
    fn apply(&mut self) {
        let preference = match self.preference() {
            Ok(p) => p,
            Err(e) => {
                set(control(self.hwnd, STATUS), &e);
                return;
            }
        };
        if let Err(e) = preference.save(&index_path().with_extension("preferences")) {
            set(
                control(self.hwnd, STATUS),
                &format!("Не удалось сохранить выбор: {e}"),
            );
            return;
        }
        self.choice = Some(preference.mode);
        self.busy("Загрузка выбранного режима…");
        let task = if self.plan_saved {
            Task::Load(preference.mode)
        } else {
            Task::Build(self.plan_root.clone(), preference.mode)
        };
        let _ = self.tasks.send(task);
    }
    fn busy(&mut self, message: &str) {
        self.ready = false;
        self.next = None;
        self.rows.clear();
        self.render_rows();
        self.generation.fetch_add(1, Ordering::Relaxed);
        for id in [
            BUILD, ALL, LOAD, NEXT, QUERY, MODE, PROFILE, BUDGET, ANALYZE, APPLY,
        ] {
            unsafe {
                EnableWindow(control(self.hwnd, id), 0);
            }
        }
        set(control(self.hwnd, STATUS), message);
    }
    fn search(&mut self, after: Option<u32>) {
        if !self.ready {
            return;
        }
        let mut query = text(control(self.hwnd, QUERY));
        let extension = text(control(self.hwnd, EXTENSION));
        if !extension.trim().is_empty() {
            query.push_str(&format!(" ext:{}", extension.trim()));
        }
        let category = unsafe { SendMessageW(control(self.hwnd, CATEGORY), 0x147, 0, 0) };
        if let Some(category) = [
            "all",
            "documents",
            "images",
            "video",
            "audio",
            "archives",
            "apps",
            "folders",
        ]
        .get(category as usize)
            && *category != "all"
        {
            query.push_str(&format!(" type:{category}"));
        }
        if unsafe { SendMessageW(control(self.hwnd, RANK), 0xf0, 0, 0) } == 1 {
            query.push_str(" rank:on");
        }
        let version = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.next = None;
        unsafe {
            EnableWindow(control(self.hwnd, NEXT), 0);
        }
        if after.is_none() {
            self.rows.clear();
            self.render_rows();
        }
        let mode = match unsafe { SendMessageW(control(self.hwnd, MODE), 0x147, 0, 0) } {
            1 => Mode::Prefix,
            2 => Mode::Exact,
            _ => Mode::Substring,
        };
        set(control(self.hwnd, STATUS), "Поиск…");
        let _ = self.tasks.send(Task::Search {
            text: query,
            mode,
            after,
            generation: version,
            scope: self.scope.clone(),
        });
    }
    fn events(&mut self) {
        while let Ok(event) = self.events.try_recv() {
            match event {
                Event::Downloaded(result) => {
                    let cancelled = self
                        .download_cancel
                        .take()
                        .is_some_and(|flag| flag.load(Ordering::Relaxed));
                    if self.closing {
                        if self.export_cancel.is_none() {
                            unsafe {
                                DestroyWindow(self.hwnd);
                            }
                        }
                        continue;
                    }
                    self.refresh_windirstat();
                    if cancelled {
                        set(
                            control(self.hwnd, STATUS),
                            "Загрузка отменена. Уже проверенный пакет, если он готов, сохранён без установки.",
                        );
                        continue;
                    }
                    match result {
                        Ok(package) => {
                            let prompt = format!(
                                "WinDirStat {}\nОфициальный SHA-256 совпал:\n{}\n{}\n\nЗапустить установщик? Windows может запросить UAC. Автоматической установки нет.",
                                package.version,
                                package.sha256,
                                if package.cached {
                                    "Используется сохранённый пакет; повторной загрузки нет."
                                } else {
                                    "Пакет скачан с официального GitHub-репозитория."
                                }
                            );
                            if unsafe {
                                MessageBoxW(
                                    self.hwnd,
                                    wide(&prompt).as_ptr(),
                                    wide("Установка WinDirStat").as_ptr(),
                                    MB_YESNO | MB_ICONQUESTION,
                                )
                            } == IDYES
                            {
                                let code = unsafe {
                                    ShellExecuteW(
                                        self.hwnd,
                                        wide("open").as_ptr(),
                                        wide(&package.path.to_string_lossy()).as_ptr(),
                                        null(),
                                        null(),
                                        SW_SHOWNORMAL,
                                    )
                                };
                                if code as isize <= 32 {
                                    set(
                                        control(self.hwnd, STATUS),
                                        "Не удалось открыть установщик. Пакет сохранён; можно повторить позже.",
                                    );
                                }
                            } else {
                                set(
                                    control(self.hwnd, STATUS),
                                    "Установка отменена. Проверенный пакет сохранён для повторного открытия.",
                                );
                            }
                        }
                        Err(e) => set(control(self.hwnd, STATUS), &e),
                    }
                }
                Event::Icons(icons) => {
                    self.icons = icons.indices;
                    for list in [control(self.hwnd, OUTPUT), unsafe {
                        GetDlgItem(palette(self.hwnd), OUTPUT as i32)
                    }] {
                        if !list.is_null() {
                            unsafe {
                                SendMessageW(
                                    list,
                                    LVM_SETIMAGELIST,
                                    LVSIL_SMALL as usize,
                                    icons.small as isize,
                                );
                                SendMessageW(
                                    list,
                                    LVM_SETIMAGELIST,
                                    LVSIL_NORMAL as usize,
                                    icons.large as isize,
                                );
                                InvalidateRect(list, null(), 0);
                            }
                        }
                    }
                }
                Event::Freshness(status, changed) => {
                    self.status = status.clone();
                    set(control(self.hwnd, STATUS), &status);
                    if changed {
                        self.search(None);
                    }
                }
                Event::Metadata(path, result) => {
                    if self.smoke.as_ref().is_some_and(|s| s.phase == 2) {
                        self.finish_smoke(
                            true,
                            if result.contains("Метаданные недоступны") {
                                "asynchronous unavailable-metadata response handled"
                            } else {
                                "asynchronous metadata available"
                            },
                        );
                    }
                    if self.selected().is_some_and(|row| row.path == path) {
                        set(control(self.hwnd, DETAILS), &result);
                    }
                }
                Event::ExportProgress(n, total) => set(
                    control(self.hwnd, STATUS),
                    &format!("Анализ метаданных: {n}/{total}. Можно отменить; поиск доступен."),
                ),
                Event::ExportDone(result) => {
                    self.export_cancel = None;
                    if self.closing {
                        if self.download_cancel.is_none() {
                            unsafe {
                                DestroyWindow(self.hwnd);
                            }
                        }
                        continue;
                    }
                    match *result {
                        Ok((path, s)) => set(
                            control(self.hwnd, STATUS),
                            &format!(
                                "Отчёт: {}. Файлов: {}, логических байт: {}, пропущено: {}, ошибок: {}, отменён: {}",
                                path.display(),
                                s.totals.files,
                                s.totals.logical_bytes,
                                s.omitted,
                                s.inaccessible,
                                s.cancelled
                            ),
                        ),
                        Err(e) => set(control(self.hwnd, STATUS), &format!("Ошибка экспорта: {e}")),
                    }
                }
                Event::Plan(summary, source, existing, root) => {
                    self.plan_saved = existing;
                    self.plan_root = root;
                    set(
                        control(self.hwnd, PLAN),
                        &format!(
                            "{}\r\n{source}\r\nЦель — поиск до 200 мс. Бюджет ориентировочный, не жёсткий лимит процесса.",
                            summary.description()
                        ),
                    );
                    self.plan = Some(summary);
                    self.ready = false;
                    for id in [PROFILE, BUDGET, ANALYZE, APPLY] {
                        unsafe {
                            EnableWindow(control(self.hwnd, id), 1);
                        }
                    }
                    set(
                        control(self.hwnd, STATUS),
                        "Выберите расход памяти и нажмите «Применить и загрузить». Поисковый индекс ещё не загружен.",
                    );
                    if let Some(p) = self.saved.take() {
                        let selected = if p.budget_mb.is_some() {
                            3
                        } else {
                            match p.mode {
                                MemoryMode::Economy => 0,
                                MemoryMode::Balanced => 1,
                                MemoryMode::Fast => 2,
                            }
                        };
                        unsafe {
                            SendMessageW(control(self.hwnd, PROFILE), 0x14e, selected, 0);
                        }
                        if let Some(mb) = p.budget_mb {
                            set(control(self.hwnd, BUDGET), &mb.to_string());
                        }
                        self.apply();
                    }
                }
                Event::Ready {
                    count,
                    root,
                    status,
                } => {
                    self.object_count = count;
                    self.ready = true;
                    self.setup = false;
                    layout(self.hwnd, false);
                    self.status = status;
                    set(control(self.hwnd, ROOT), &root);
                    set(control(self.hwnd, STATUS), &self.status);
                    for id in [
                        BUILD, ALL, LOAD, QUERY, MODE, PROFILE, BUDGET, ANALYZE, APPLY,
                    ] {
                        unsafe {
                            EnableWindow(control(self.hwnd, id), 1);
                        }
                    }
                    set(
                        self.hwnd,
                        &format!("HyperSearch 0.2 beta — {count} объектов"),
                    );
                    set(
                        control(self.hwnd, ADDRESS),
                        self.scope.as_deref().unwrap_or("Этот компьютер"),
                    );
                    self.search(None);
                    if let Some(smoke) = &mut self.smoke
                        && smoke.phase == 0
                    {
                        smoke.log.push(format!(
                            "application_ready_ms={:.3}; entries={count}",
                            smoke.start.elapsed().as_secs_f64() * 1000.
                        ));
                        smoke.log.push(format!(
                            "gui_process_metrics={:?}",
                            hypersearch_engine::metrics::process()
                        ));
                        smoke.phase = 1;
                        smoke.query = Instant::now();
                        set(control(self.hwnd, QUERY), "excel.exe");
                        self.search(None);
                    }
                }
                Event::Error(e) => {
                    self.ready = false;
                    self.status = e.clone();
                    set(control(self.hwnd, STATUS), &e);
                    for id in [ANALYZE, PROFILE, BUDGET] {
                        unsafe {
                            EnableWindow(control(self.hwnd, id), 1);
                        }
                    }
                    if self.choice.is_some() {
                        for id in [BUILD, ALL, LOAD] {
                            unsafe {
                                EnableWindow(control(self.hwnd, id), 1);
                            }
                        }
                    }
                    if self.plan.is_some() {
                        unsafe {
                            EnableWindow(control(self.hwnd, APPLY), 1);
                        }
                    }
                }
                Event::Results {
                    generation,
                    rows,
                    next,
                    status,
                } => {
                    if generation != self.generation.load(Ordering::Relaxed) {
                        continue;
                    }
                    self.rows.extend(rows);
                    self.last_search = status.clone();
                    self.sort_loaded();
                    let keys: Vec<_> = self
                        .rows
                        .iter()
                        .map(|r| super::icons::key(&r.name, r.directory))
                        .collect::<std::collections::HashSet<_>>()
                        .into_iter()
                        .take(256)
                        .collect();
                    let _ = self.tasks.send(Task::Icons(keys));
                    self.next = next;
                    self.render_rows();
                    if self.smoke.as_ref().is_some_and(|s| s.phase == 1) {
                        self.inspect_smoke();
                    }
                    set(
                        control(self.hwnd, STATUS),
                        &format!("{status}\r\n{}", self.status),
                    );
                    unsafe {
                        EnableWindow(control(self.hwnd, NEXT), next.is_some() as i32);
                    }
                }
            }
        }
    }
}
fn child(parent: HWND, class: &str, title: &str, style: u32, id: usize) -> HWND {
    unsafe {
        let h = CreateWindowExW(
            if class == "EDIT" { WS_EX_CLIENTEDGE } else { 0 },
            wide(class).as_ptr(),
            wide(title).as_ptr(),
            WS_CHILD | WS_VISIBLE | style,
            0,
            0,
            0,
            0,
            parent,
            id as HMENU,
            GetModuleHandleW(null()),
            null(),
        );
        SendMessageW(h, WM_SETFONT, ui_font(GetDpiForWindow(parent).max(96)), 1);
        h
    }
}
fn layout(hwnd: HWND, setup: bool) {
    unsafe {
        for id in [
            WDS,
            WDS_PATH,
            WDS_SAVE,
            WDS_SITE,
            WDS_UPDATE,
            WDS_LABEL,
            THEME,
            GLASS,
            EXPORT,
            AI_REPORT,
            CANCEL_EXPORT,
            EXPORT_FORMAT,
            SAFE_EXPORT,
            HOTKEY,
            HOTKEY_SAVE,
            HOTKEY_LABEL,
            COPY_PROMPT,
        ] {
            ShowWindow(control(hwnd, id), if setup { SW_SHOW } else { SW_HIDE });
        }
        let mut r: RECT = std::mem::zeroed();
        GetClientRect(hwnd, &mut r);
        let dpi = GetDpiForWindow(hwnd).max(96) as i32;
        let w = (r.right * 96 / dpi).max(1000);
        let h = (r.bottom * 96 / dpi).max(580);
        for id in [
            201, ROOT, BUILD, ALL, 203, PROFILE, 204, BUDGET, ANALYZE, APPLY, PLAN,
        ] {
            ShowWindow(control(hwnd, id), if setup { SW_SHOW } else { SW_HIDE });
        }
        ShowWindow(control(hwnd, 202), SW_HIDE);
        let settings = GetPropW(hwnd, wide("HyperSearch.Settings").as_ptr());
        if !settings.is_null() {
            let smoke = !GetPropW(hwnd, wide("HyperSearch.Smoke").as_ptr()).is_null();
            ShowWindow(settings, if setup && !smoke { SW_SHOW } else { SW_HIDE });
        }
        let list_top = 138;
        for (id, x, y, cw, ch) in [
            (BACK, 12, 12, 34, 30),
            (FORWARD, 50, 12, 34, 30),
            (UP, 88, 12, 34, 30),
            (LOAD, 128, 12, 64, 30),
            (VIEW, 400, 54, 72, 30),
            (PIN, 480, 54, 90, 30),
            (SPOTLIGHT, 12, 54, 178, 30),
            (COPY, 578, 54, 82, 30),
            (DETAILS, w - 270, 138, 258, h - 238),
            (CATEGORY, 204, 96, 155, 240),
            (EXTENSION, 369, 96, 125, 30),
            (RANK, 504, 96, 170, 30),
            (SECTIONS, 12, 54, 178, 480),
            (ABOUT, 204, 54, 770, 400),
            (SEARCH_HELP, 204, 54, 770, 300),
            (PERF_HELP, 204, 54, 770, 300),
            (ANIMATIONS, 204, 270, 260, 30),
            (COPY_PROMPT, 744, 510, 240, 28),
            (HOTKEY_LABEL, 204, 480, 550, 24),
            (HOTKEY, 204, 510, 280, 28),
            (HOTKEY_SAVE, 494, 510, 240, 28),
            (ADDRESS, 204, 12, w - 554, 30),
            (GO, w - 342, 12, 50, 30),
            (QUERY, w - 280, 12, 268, 30),
            (SIDEBAR, 12, 96, 178, h - 196),
            (205, 12, 62, 178, 24),
            (OPEN, 204, 54, 88, 30),
            (LOCATION, 300, 54, 92, 30),
            (SETTINGS, w - 184, 54, 172, 30),
            (MODE, w - 364, 54, 172, 200),
            (201, 204, 108, 76, 24),
            (ROOT, 280, 102, w - 684, 30),
            (BUILD, w - 390, 102, 130, 30),
            (ALL, w - 252, 102, 122, 30),
            (203, 204, 148, 76, 24),
            (PROFILE, 280, 142, 220, 220),
            (204, 514, 148, 100, 24),
            (BUDGET, 614, 142, 72, 30),
            (ANALYZE, 204, 182, 140, 30),
            (APPLY, 354, 182, 230, 30),
            (PLAN, 204, 224, w - 216, 110),
            (WDS_LABEL, 204, 330, w - 216, 24),
            (WDS, 204, 356, 180, 28),
            (WDS_SITE, 394, 356, 160, 28),
            (WDS_UPDATE, 564, 356, 190, 28),
            (WDS_PATH, 204, 390, w - 390, 28),
            (WDS_SAVE, w - 174, 390, 162, 28),
            (THEME, 780, 182, 180, 28),
            (GLASS, 780, 224, 180, 28),
            (EXPORT, 204, 432, 160, 28),
            (AI_REPORT, 374, 432, 160, 28),
            (CANCEL_EXPORT, 544, 432, 100, 28),
            (EXPORT_FORMAT, 654, 432, 115, 200),
            (SAFE_EXPORT, 780, 432, 210, 28),
            (OUTPUT, 204, list_top, w - 486, h - list_top - 100),
            (STATUS, 12, h - 88, w - 160, 80),
            (NEXT, w - 140, h - 56, 128, 32),
        ] {
            let parent = GetParent(control(hwnd, id));
            let local_dpi = GetDpiForWindow(parent).max(96) as i32;
            MoveWindow(
                control(hwnd, id),
                x * local_dpi / 96,
                y * local_dpi / 96,
                cw * local_dpi / 96,
                ch * local_dpi / 96,
                1,
            );
        }
        let section = SendMessageW(control(hwnd, SECTIONS), 0x188, 0, 0);
        if !settings.is_null() {
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(settings, &mut r);
            let sdpi = GetDpiForWindow(settings).max(96) as i32;
            let s = |v: i32| v * sdpi / 96;
            let sw = (r.right * 96 / sdpi).max(1000);
            for (id, x, y, width, height) in [
                (THEME, 204, 54, 260, 30),
                (GLASS, 204, 100, 260, 30),
                (ANIMATIONS, 204, 146, 260, 30),
                (201, 204, 54, 70, 24),
                (ROOT, 280, 50, sw - 600, 30),
                (BUILD, sw - 300, 50, 130, 30),
                (ALL, sw - 160, 50, 140, 30),
                (203, 204, 100, 70, 24),
                (PROFILE, 280, 94, 220, 220),
                (204, 514, 100, 100, 24),
                (BUDGET, 614, 94, 72, 30),
                (ANALYZE, 204, 140, 140, 30),
                (APPLY, 354, 140, 230, 30),
                (PLAN, 204, 190, sw - 230, 220),
                (WDS_LABEL, 204, 54, sw - 230, 30),
                (WDS, 204, 90, 180, 30),
                (WDS_SITE, 394, 90, 160, 30),
                (WDS_UPDATE, 564, 90, 190, 30),
                (WDS_PATH, 204, 130, sw - 410, 30),
                (WDS_SAVE, sw - 194, 130, 174, 30),
                (EXPORT, 204, 190, 160, 30),
                (AI_REPORT, 374, 190, 160, 30),
                (CANCEL_EXPORT, 544, 190, 100, 30),
                (EXPORT_FORMAT, 654, 190, 115, 200),
                (SAFE_EXPORT, 204, 234, 280, 30),
                (COPY_PROMPT, 204, 280, 260, 30),
                (HOTKEY_LABEL, 204, 54, sw - 230, 30),
                (HOTKEY, 204, 94, 280, 30),
                (HOTKEY_SAVE, 494, 94, 240, 30),
            ] {
                MoveWindow(control(hwnd, id), s(x), s(y), s(width), s(height), 1);
            }
            for (id, x, y, width, height) in [
                (EXPORT_HELP, 204, 172, sw - 230, 44),
                (EXPORT_ROOT, 204, 222, 420, 200),
                (EXPORT_FORMAT, 640, 222, 140, 200),
                (EXPORT_DEPTH, 204, 264, 72, 30),
                (EXPORT_MIN, 290, 264, 130, 30),
                (EXPORT_NEUTRAL, 434, 264, 270, 30),
                (SAFE_EXPORT, 204, 306, 280, 30),
                (EXPORT_COMPACT, 504, 306, 230, 30),
                (EXPORT_EXCLUDE, 204, 350, sw - 230, 30),
                (EXPORT, 204, 396, 160, 30),
                (AI_REPORT, 374, 396, 160, 30),
                (CANCEL_EXPORT, 544, 396, 100, 30),
                (COPY_PROMPT, 204, 444, 260, 30),
            ] {
                MoveWindow(control(hwnd, id), s(x), s(y), s(width), s(height), 1);
            }
        }
        for (page, ids) in [
            (0, vec![THEME, GLASS, ANIMATIONS]),
            (1, vec![SEARCH_HELP]),
            (
                2,
                vec![
                    201, ROOT, BUILD, ALL, 203, PROFILE, 204, BUDGET, ANALYZE, APPLY, PLAN,
                ],
            ),
            (
                3,
                vec![
                    WDS_LABEL,
                    WDS,
                    WDS_SITE,
                    WDS_UPDATE,
                    WDS_PATH,
                    WDS_SAVE,
                    EXPORT,
                    AI_REPORT,
                    CANCEL_EXPORT,
                    EXPORT_FORMAT,
                    SAFE_EXPORT,
                    COPY_PROMPT,
                    EXPORT_ROOT,
                    EXPORT_DEPTH,
                    EXPORT_MIN,
                    EXPORT_NEUTRAL,
                    EXPORT_EXCLUDE,
                    EXPORT_COMPACT,
                    EXPORT_HELP,
                ],
            ),
            (4, vec![HOTKEY_LABEL, HOTKEY, HOTKEY_SAVE]),
            (5, vec![PERF_HELP]),
            (6, vec![ABOUT]),
        ] {
            for id in ids {
                ShowWindow(
                    control(hwnd, id),
                    if setup && section == page {
                        SW_SHOW
                    } else {
                        SW_HIDE
                    },
                );
            }
        }
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_DPICHANGED && l != 0 {
        let rect = unsafe { &*(l as *const RECT) };
        unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
            EnumChildWindows(hwnd, Some(update_font), (w & 0xffff) as isize);
            let owner = GetWindow(hwnd, GW_OWNER);
            if !owner.is_null() {
                SendMessageW(owner, WM_SIZE, 0, 0);
            }
        }
        return 0;
    }
    let owner = unsafe { GetWindow(hwnd, GW_OWNER) };
    if !owner.is_null() {
        let is_palette = palette(owner) == hwnd;
        match msg {
            WM_CLOSE => {
                if !is_palette {
                    APP.with(|a| {
                        if let Ok(mut a) = a.try_borrow_mut()
                            && let Some(a) = a.as_mut()
                        {
                            a.setup = false;
                        }
                    });
                }
                unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                }
                return 0;
            }
            WM_DESTROY => return 0,
            WM_GETMINMAXINFO if is_palette => return unsafe { DefWindowProcW(hwnd, msg, w, l) },
            WM_SIZE if is_palette => {
                let mut r: RECT = unsafe { std::mem::zeroed() };
                unsafe {
                    GetClientRect(hwnd, &mut r);
                    let dpi = GetDpiForWindow(hwnd).max(96) as i32;
                    let s = |v: i32| v * dpi / 96;
                    MoveWindow(
                        GetDlgItem(hwnd, QUERY as i32),
                        s(20),
                        s(18),
                        (r.right - s(40)).max(100),
                        s(38),
                        1,
                    );
                    MoveWindow(
                        GetDlgItem(hwnd, OUTPUT as i32),
                        s(20),
                        s(72),
                        (r.right - s(40)).max(100),
                        (r.bottom - s(92)).max(60),
                        1,
                    );
                }
                return 0;
            }
            WM_SIZE => return 0,
            WM_COMMAND
                if is_palette
                    && w & 0xffff == QUERY
                    && (w >> 16) & 0xffff == EN_CHANGE as usize =>
            {
                set(
                    control(owner, QUERY),
                    &text(unsafe { GetDlgItem(hwnd, QUERY as i32) }),
                );
                return 0;
            }
            WM_COMMAND | WM_NOTIFY => return unsafe { SendMessageW(owner, msg, w, l) },
            _ => {}
        }
    }
    match msg {
        WM_CLOSE => {
            let waiting = APP.with(|a| {
                if let Ok(mut a) = a.try_borrow_mut()
                    && let Some(a) = a.as_mut()
                {
                    a.closing = true;
                    if let Some(cancel) = &a.download_cancel {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    if let Some(cancel) = &a.export_cancel {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    return a.download_cancel.is_some() || a.export_cancel.is_some();
                }
                false
            });
            if !waiting {
                unsafe {
                    DestroyWindow(hwnd);
                }
            } else {
                set(
                    control(hwnd, STATUS),
                    "Закрытие после отмены загрузки/экспорта…",
                );
            }
            return 0;
        }
        WM_SETTINGCHANGE | WM_THEMECHANGED | WM_DWMCOMPOSITIONCHANGED => {
            let look = LOOK.with(|look| look.get());
            look.apply(hwnd);
            let settings = unsafe { GetPropW(hwnd, wide("HyperSearch.Settings").as_ptr()) };
            if !settings.is_null() {
                look.apply(settings);
            }
            if !palette(hwnd).is_null() {
                look.apply(palette(hwnd));
            }
        }
        WM_TIMER if w == 3 => {
            APP.with(|a| { if let Ok(mut a)=a.try_borrow_mut() && let Some(a)=a.as_mut() {
                if let Some(smoke)=&mut a.smoke {
                    let wall=smoke.query.elapsed().as_secs_f64();
                    let cpu=hypersearch_engine::metrics::process().map(|m|m.cpu_seconds-smoke.idle_cpu).unwrap_or(0.);
                    smoke.log.push(format!("hidden_idle_wall_seconds={wall:.3}; process_cpu_seconds={cpu:.6}; one_core_equivalent_percent={:.4}",cpu/wall*100.));
                }
                unsafe { KillTimer(hwnd,3); }
                a.finish_smoke(true,"hidden idle completed");
            } });
            return 0;
        }
        WM_TIMER if w == 2 => {
            APP.with(|a| {
                if let Ok(mut a) = a.try_borrow_mut()
                    && let Some(a) = a.as_mut()
                {
                    a.finish_smoke(false, "GUI diagnostic timeout");
                }
            });
            return 0;
        }
        WM_TIMER if w == 1 => {
            APP.with(|a| {
                if let Ok(a) = a.try_borrow()
                    && let Some(a) = a.as_ref()
                    && a.ready
                {
                    let _ = a.tasks.send(Task::Refresh);
                }
            });
            return 0;
        }
        WM_CONTEXTMENU
            if w as HWND == control(hwnd, OUTPUT)
                || w as HWND == unsafe { GetDlgItem(palette(hwnd), OUTPUT as i32) } =>
        {
            unsafe {
                let menu = CreatePopupMenu();
                for (id, label) in [
                    (OPEN, "Открыть"),
                    (LOCATION, "Открыть расположение"),
                    (COPY, "Копировать путь"),
                    (PIN, "Закрепить текущую папку"),
                    (WDS_SCOPE, "Анализировать в WinDirStat"),
                ] {
                    AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
                }
                let mut point = POINT { x: 0, y: 0 };
                GetCursorPos(&mut point);
                let chosen = TrackPopupMenu(
                    menu,
                    TPM_RETURNCMD | TPM_RIGHTBUTTON,
                    point.x,
                    point.y,
                    0,
                    hwnd,
                    null(),
                );
                DestroyMenu(menu);
                if chosen != 0 {
                    SendMessageW(hwnd, WM_COMMAND, chosen as usize, 0);
                }
            }
            return 0;
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX | WM_CTLCOLORBTN => {
            let look = LOOK.with(|look| look.get());
            let (bg, fg) = if look.dark() {
                (0x00282020, 0x00f2f2f2)
            } else {
                (0x00faf7f4, 0x00282020)
            };
            unsafe {
                SetTextColor(w as HDC, fg);
                if msg == WM_CTLCOLORSTATIC
                    && !GetPropW(hwnd, wide("HyperSearch.Glass").as_ptr()).is_null()
                {
                    SetBkMode(w as HDC, TRANSPARENT as i32);
                    return GetStockObject(NULL_BRUSH) as isize;
                }
                SetBkColor(w as HDC, bg);
                let brush = GetStockObject(DC_BRUSH);
                SetDCBrushColor(w as HDC, bg);
                return brush as isize;
            }
        }
        WM_ERASEBKGND => {
            let look = LOOK.with(|look| look.get());
            let bg = if unsafe { !GetPropW(hwnd, wide("HyperSearch.Glass").as_ptr()).is_null() } {
                0
            } else if look.dark() {
                0x00282020
            } else {
                0x00faf7f4
            };
            unsafe {
                let mut rect: RECT = std::mem::zeroed();
                GetClientRect(hwnd, &mut rect);
                SetDCBrushColor(w as HDC, bg);
                FillRect(w as HDC, &rect, GetStockObject(DC_BRUSH) as HBRUSH);
            }
            return 1;
        }
        WM_HOTKEY => {
            APP.with(|a| {
                if let Ok(mut a) = a.try_borrow_mut()
                    && let Some(a) = a.as_mut()
                {
                    a.navigate(None, true);
                }
            });
            show_palette(hwnd);
            return 0;
        }
        WM_SIZE => {
            let setup = APP.with(|a| {
                a.try_borrow()
                    .ok()
                    .and_then(|a| a.as_ref().map(|a| a.setup))
                    .unwrap_or(false)
            });
            layout(hwnd, setup);
            return 0;
        }
        WM_GETMINMAXINFO => {
            if l != 0 {
                let info = unsafe { &mut *(l as *mut MINMAXINFO) };
                let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as i32;
                info.ptMinTrackSize = POINT {
                    x: 1040 * dpi / 96,
                    y: 660 * dpi / 96,
                };
            }
            return 0;
        }
        WM_NOTIFY => {
            if l != 0 {
                let notify = unsafe { &*(l as *const NMHDR) };
                if notify.idFrom == OUTPUT && notify.code == LVN_ITEMCHANGED {
                    APP.with(|a| {
                        if let Ok(a) = a.try_borrow()
                            && let Some(a) = a.as_ref()
                            && let Some(row) = a.selected()
                        {
                            let _ = a.tasks.send(Task::Metadata(row.path));
                        }
                    });
                }
                if notify.idFrom == OUTPUT && notify.code == LVN_GETDISPINFOW {
                    let info = unsafe { &mut *(l as *mut NMLVDISPINFOW) };
                    APP.with(|a| {
                        if let Ok(a) = a.try_borrow()
                            && let Some(a) = a.as_ref()
                            && let Some(row) = a.rows.get(info.item.iItem as usize)
                            && info.item.mask & LVIF_TEXT != 0
                            && !info.item.pszText.is_null()
                            && info.item.cchTextMax > 0
                        {
                            let value = match info.item.iSubItem {
                                1 => &row.path,
                                2 => &row.kind,
                                _ => &row.name,
                            };
                            let value = wide(value);
                            let len = (info.item.cchTextMax as usize - 1).min(value.len() - 1);
                            unsafe {
                                std::ptr::copy_nonoverlapping(
                                    value.as_ptr(),
                                    info.item.pszText,
                                    len,
                                );
                                *info.item.pszText.add(len) = 0;
                            }
                        }
                        if let Ok(a) = a.try_borrow()
                            && let Some(a) = a.as_ref()
                            && let Some(row) = a.rows.get(info.item.iItem as usize)
                            && info.item.mask & LVIF_IMAGE != 0
                        {
                            info.item.iImage = a
                                .icons
                                .get(&super::icons::key(&row.name, row.directory))
                                .copied()
                                .unwrap_or(0);
                        }
                    });
                }
                if notify.idFrom == OUTPUT && notify.code == LVN_COLUMNCLICK {
                    let column = unsafe { (*(l as *const NMLISTVIEW)).iSubItem };
                    APP.with(|a| {
                        if let Ok(mut a) = a.try_borrow_mut()
                            && let Some(a) = a.as_mut()
                        {
                            let list = control(hwnd, OUTPUT);
                            unsafe {
                                let same = GetPropW(list, wide("HyperSearch.SortColumn").as_ptr())
                                    as usize
                                    == column as usize + 1;
                                let descending =
                                    GetPropW(list, wide("HyperSearch.SortDescending").as_ptr())
                                        as usize
                                        != 0;
                                SetPropW(
                                    list,
                                    wide("HyperSearch.SortColumn").as_ptr(),
                                    (column as usize + 1) as _,
                                );
                                SetPropW(
                                    list,
                                    wide("HyperSearch.SortDescending").as_ptr(),
                                    (same && !descending) as usize as _,
                                );
                            }
                            a.sort_loaded();
                            a.render_rows();
                        }
                    });
                }
                if notify.idFrom == OUTPUT && notify.code == NM_DBLCLK {
                    APP.with(|a| {
                        if let Ok(mut app) = a.try_borrow_mut()
                            && let Some(app) = app.as_mut()
                        {
                            app.open(false);
                        }
                    });
                }
            }
            return 0;
        }
        WM_DESTROY => {
            unsafe {
                PostQuitMessage(0);
            }
            return 0;
        }
        WM_COMMAND | DONE => {
            APP.with(|cell| {
                let Ok(mut app) = cell.try_borrow_mut() else {
                    return;
                };
                let Some(app) = app.as_mut() else {
                    return;
                };
                if msg == DONE {
                    app.events();
                    return;
                }
                if app.closing { return; }
                let id = w & 0xffff;
                let event = (w >> 16) & 0xffff;
                match (id, event) {
                    (SECTIONS, 1) => {
                        app.refresh_performance();
                        layout(hwnd, true);
                    }
                    (RANK, 0) => {
                        unsafe {
                            RemovePropW(
                                control(hwnd, OUTPUT),
                                wide("HyperSearch.SortColumn").as_ptr(),
                            );
                        }
                        app.search(None);
                    }
                    (EXTENSION, 0x300) | (CATEGORY, 1) => app.search(None),
                    (ANIMATIONS, 0) => {
                        let mut a = LOOK.with(|look| look.get());
                        a.animations = !a.animations;
                        if a.save(&index_path().with_extension("appearance")).is_ok() {
                            LOOK.with(|look| look.set(a));
                        }
                        set(
                            control(hwnd, ANIMATIONS),
                            if a.animations {
                                "Анимации: вкл"
                            } else {
                                "Анимации: выкл"
                            },
                        );
                    }
                    (COPY, 0) => {
                        let list = if unsafe { IsWindowVisible(palette(hwnd)) } != 0 {
                            unsafe { GetDlgItem(palette(hwnd), OUTPUT as i32) }
                        } else {
                            control(hwnd, OUTPUT)
                        };
                        let mut i = -1isize;
                        let mut paths = Vec::new();
                        loop {
                            i = unsafe {
                                SendMessageW(
                                    list,
                                    LVM_GETNEXTITEM,
                                    i as usize,
                                    LVNI_SELECTED as isize,
                                )
                            };
                            if i < 0 {
                                break;
                            }
                            if let Some(row) = app.rows.get(i as usize) {
                                paths.push(row.path.clone());
                            }
                        }
                        if let Err(e) = copy_text(hwnd, &paths.join("\r\n")) {
                            set(control(hwnd, STATUS), &e);
                        }
                    }
                    (COPY_PROMPT, 0) => {
                        if let Err(e) =
                            copy_text(hwnd, hypersearch_engine::storage_analysis::AI_PROMPT)
                        {
                            set(control(hwnd, STATUS), &e);
                        }
                    }
                    (SPOTLIGHT, 0) => {
                        app.navigate(None, true);
                        show_palette(hwnd);
                    }
                    (WDS_SCOPE, 0) => {
                        if let Some(executable) = super::windirstat::installed() {
                            let target = app
                                .selected()
                                .filter(|r| r.directory)
                                .map(|r| r.path)
                                .or_else(|| app.scope.clone());
                            if let Some(target) = target {
                                if let Err(e) =
                                    std::process::Command::new(executable).arg(target).spawn()
                                {
                                    set(control(hwnd, STATUS), &e.to_string());
                                }
                            } else {
                                set(
                                    control(hwnd, STATUS),
                                    "Выберите диск или папку для анализа.",
                                );
                            }
                        } else {
                            set(
                                control(hwnd, STATUS),
                                "WinDirStat не найден. Откройте Хранилище в параметрах.",
                            );
                        }
                    }
                    (HOTKEY_SAVE, 0) => {
                        let value = text(control(hwnd, HOTKEY));
                        match register_shortcut(hwnd, &value) {
                            Ok(()) => {
                                match std::fs::write(index_path().with_extension("hotkey"), value) {
                                    Ok(()) => set(
                                        control(hwnd, HOTKEY_LABEL),
                                        "Сочетание сохранено. off отключает глобальную клавишу.",
                                    ),
                                    Err(e) => set(control(hwnd, HOTKEY_LABEL), &e.to_string()),
                                }
                            }
                            Err(e) => set(control(hwnd, HOTKEY_LABEL), &e),
                        }
                    }
                    (EXPORT, 0) => app.export(false),
                    (AI_REPORT, 0) => app.export(true),
                    (CANCEL_EXPORT, 0) => {
                        if let Some(cancel) = &app.export_cancel {
                            cancel.store(true, Ordering::Relaxed);
                            set(control(hwnd, STATUS), "Отмена анализа…");
                        }
                    }
                    (THEME, 0) | (GLASS, 0) => {
                        let path = index_path().with_extension("appearance");
                        let mut a = super::appearance::Appearance::load(&path);
                        if id == THEME {
                            a.theme = match a.theme {
                                super::appearance::Theme::System => super::appearance::Theme::Light,
                                super::appearance::Theme::Light => super::appearance::Theme::Dark,
                                super::appearance::Theme::Dark => super::appearance::Theme::System,
                            };
                        } else {
                            a.transparency = !a.transparency;
                        }
                        match a.save(&path) {
                            Ok(()) => {
                                LOOK.with(|look| look.set(a));
                                a.apply(hwnd);
                                unsafe {
                                    a.apply(GetPropW(hwnd, wide("HyperSearch.Settings").as_ptr()));
                                }
                                a.apply(palette(hwnd));
                                set(control(hwnd, THEME), &format!("Тема: {:?}", a.theme));
                                set(
                                    control(hwnd, GLASS),
                                    if a.transparency {
                                        "Прозрачность: вкл"
                                    } else {
                                        "Прозрачность: выкл"
                                    },
                                );
                            }
                            Err(e) => set(control(hwnd, STATUS), &e.to_string()),
                        }
                    }
                    (SETTINGS, 0) => {
                        app.setup = !app.setup;
                        if app.setup {
                            app.refresh_windirstat();
                            app.refresh_performance();
                        }
                        layout(hwnd, app.setup);
                    }
                    (WDS, 0) => {
                        if let Some(cancel)=&app.download_cancel { cancel.store(true,Ordering::Relaxed); return; }
                        if super::windirstat::installed().is_none() && (super::windirstat_download::available() || super::windirstat::downloaded().is_none()) {
                            let cancel=Arc::new(AtomicBool::new(false));
                            app.download_cancel=Some(cancel.clone());
                            let _=app.tasks.send(Task::Download(cancel));
                            set(control(hwnd,WDS),"Отменить загрузку");
                            set(control(hwnd,STATUS),"Загрузка/проверка WinDirStat вне UI. Установщик не запустится без подтверждения.");
                            return;
                        }
                        let target = super::windirstat::installed()
                            .map(|p| p.to_string_lossy().into_owned())
                            .or_else(|| {
                                super::windirstat::downloaded()
                                    .map(|p| {
                                        if p.extension()
                                            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
                                        {
                                            p
                                        } else {
                                            p.parent().unwrap_or(&p).to_path_buf()
                                        }
                                    })
                                    .map(|p| p.to_string_lossy().into_owned())
                            })
                            .unwrap_or_else(|| super::windirstat::RELEASES.into());
                        let code=unsafe {
                            ShellExecuteW(
                                hwnd,
                                wide("open").as_ptr(),
                                wide(&target).as_ptr(),
                                null(),
                                null(),
                                SW_SHOWNORMAL,
                            )
                        };
                        if code as isize<=32 { set(control(hwnd,STATUS),"Не удалось открыть WinDirStat/пакет. Проверьте путь или официальный сайт."); }
                        app.refresh_windirstat();
                    }
                    (WDS_SITE, 0) | (WDS_UPDATE, 0) => {
                        let url = if id == WDS_SITE {
                            super::windirstat::WEBSITE
                        } else {
                            super::windirstat::RELEASES
                        };
                        unsafe {
                            ShellExecuteW(
                                hwnd,
                                wide("open").as_ptr(),
                                wide(url).as_ptr(),
                                null(),
                                null(),
                                SW_SHOWNORMAL,
                            );
                        }
                    }
                    (WDS_SAVE, 0) => {
                        let path = PathBuf::from(text(control(hwnd, WDS_PATH)));
                        match super::windirstat::remember(&path, &super::windirstat::state_file()) {
                            Ok(()) => app.refresh_windirstat(),
                            Err(e) => set(control(hwnd, STATUS), &e.to_string()),
                        }
                    }
                    (GO, 0) => {
                        let path = text(control(hwnd, ADDRESS));
                        app.navigate(
                            if path.trim().is_empty()
                                || path.trim() == "*"
                                || path == "Этот компьютер"
                            {
                                None
                            } else {
                                Some(path)
                            },
                            true,
                        );
                    }
                    (BACK, 0) => {
                        if let Some(previous) = app.history.pop() {
                            app.forward.push(app.scope.clone());
                            app.navigate(previous, false);
                        }
                    }
                    (FORWARD, 0) => {
                        if let Some(next) = app.forward.pop() {
                            app.history.push(app.scope.clone());
                            app.navigate(next, false);
                        }
                    }
                    (VIEW, 0) => unsafe {
                        let list = control(hwnd, OUTPUT);
                        let next = match SendMessageW(list, LVM_GETVIEW, 0, 0) as u32 {
                            LV_VIEW_DETAILS => LV_VIEW_LIST,
                            LV_VIEW_LIST => LV_VIEW_ICON,
                            _ => LV_VIEW_DETAILS,
                        };
                        SendMessageW(list, LVM_SETVIEW, next as usize, 0);
                    },
                    (PIN, 0) => {
                        if let Some(path) = app.scope.clone()
                            && !app.sidebar.contains(&Some(path.clone()))
                        {
                            unsafe {
                                SendMessageW(
                                    control(hwnd, SIDEBAR),
                                    0x180,
                                    0,
                                    wide(&path).as_ptr() as isize,
                                );
                            }
                            app.sidebar.push(Some(path));
                            let pins: Vec<_> = app
                                .sidebar
                                .iter()
                                .flatten()
                                .filter(|p| p.len() > 3)
                                .cloned()
                                .collect();
                            if let Err(e) =
                                std::fs::write(index_path().with_extension("pins"), pins.join("\n"))
                            {
                                set(control(hwnd, STATUS), &e.to_string());
                            }
                        }
                    }
                    (UP, 0) => {
                        let parent = app.scope.as_ref().and_then(|p| view_model::parent(p));
                        app.navigate(parent, true);
                    }
                    (OPEN, 0) => app.open(false),
                    (LOCATION, 0) => app.open(true),
                    (SIDEBAR, 1) => {
                        let i = unsafe { SendMessageW(control(hwnd, SIDEBAR), 0x188, 0, 0) };
                        if i >= 0
                            && let Some(scope) = app.sidebar.get(i as usize)
                        {
                            app.navigate(scope.clone(), true);
                        }
                    }
                    (ANALYZE, 0) => {
                        let root = text(control(hwnd, ROOT));
                        if root.trim().is_empty() {
                            set(
                                control(hwnd, STATUS),
                                "Укажите каталог или * для всех дисков.",
                            );
                            return;
                        }
                        app.saved = None;
                        app.choice = None;
                        app.plan = None;
                        app.busy("Подсчёт объёма памяти без загрузки поискового индекса…");
                        let _ = app.tasks.send(Task::Analyze(root));
                    }
                    (APPLY, 0) => app.apply(),
                    (ALL, 0) => {
                        let Some(mode) = app.choice else {
                            return;
                        };
                        app.busy("Индексация всех доступных дисков…");
                        let _ = app.tasks.send(Task::Build("*".into(), mode));
                    }
                    (BUILD, 0) => {
                        let Some(mode) = app.choice else {
                            return;
                        };
                        let root = text(control(hwnd, ROOT));
                        if root.trim().is_empty() {
                            set(control(hwnd, STATUS), "Укажите путь к каталогу или диску.");
                            return;
                        }
                        app.busy("Индексация… окно остаётся доступным");
                        let _ = app.tasks.send(Task::Build(root, mode));
                    }
                    (LOAD, 0) => {
                        let Some(mode) = app.choice else {
                            return;
                        };
                        app.busy("Загрузка индекса…");
                        let _ = app.tasks.send(Task::Load(mode));
                    }
                    (QUERY, 0x300) | (MODE, 1) => app.search(None),
                    (NEXT, 0) => {
                        if let Some(next) = app.next {
                            app.search(Some(next));
                        }
                    }
                    _ => {}
                }
            });
            return 0;
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}
pub fn run() {
    unsafe {
        let smoke = std::env::args().any(|arg| arg == "--self-test-ui");
        let smoke_start = Instant::now();
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let common = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES | ICC_STANDARD_CLASSES,
        };
        InitCommonControlsEx(&common);
        let instance = GetModuleHandleW(null());
        let class = wide("HyperSearchNativeWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(window_proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            hbrBackground: (COLOR_WINDOW + 1) as HBRUSH,
            ..std::mem::zeroed()
        };
        if RegisterClassW(&wc) == 0 {
            return;
        }
        let initial_dpi = GetDpiForSystem().max(96) as i32;
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("HyperSearch 0.2 beta").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1200 * initial_dpi / 96,
            800 * initial_dpi / 96,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        if smoke {
            SetPropW(hwnd, wide("HyperSearch.Smoke").as_ptr(), 1usize as _);
        }
        super::appearance::Appearance::load(&index_path().with_extension("appearance")).apply(hwnd);
        child(hwnd, "BUTTON", "Тема: System", WS_TABSTOP, THEME);
        child(hwnd, "BUTTON", "Прозрачность", WS_TABSTOP, GLASS);
        child(hwnd, "BUTTON", "Анимации: выкл", WS_TABSTOP, ANIMATIONS);
        child(
            hwnd,
            "STATIC",
            "HyperSearch 0.2 beta\r\nНативный поиск файлов Windows / Rust\r\nИндекс хранится локально. Отчёты не отправляются в сеть.\r\nЛицензия HyperSearch: в репозитории не задана.\r\nWinDirStat — отдельная программа со своей лицензией.\r\nhttps://github.com/Noki1945/HyperSearch",
            0,
            ABOUT,
        );
        child(
            hwnd,
            "STATIC",
            "Фильтры: категория, расширения через запятую, область в адресной строке.\r\nВ запросе: ext:exe,dll type:apps in:\"C:\\Work Files\" rank:on\r\nРанжирование меняет порядок текущей страницы: точное имя, начало, начало слова, подстрока.\r\nCtrl+F — поиск; Ctrl+L — адрес; Alt+стрелки — история; Ctrl+C — пути.\r\nSpotlight: стрелка вниз, Enter, Esc. Глобальная клавиша включается отдельно.",
            0,
            SEARCH_HELP,
        );
        child(
            hwnd,
            "STATIC",
            "Три режима памяти: Экономный / Сбалансированный / Быстрый.\r\nТаблицы сохраняются на диск и проверяются при повторном запуске.\r\nПоиск выполняется вне UI; устаревшие запросы отменяются.\r\nМетаданные читаются отдельным рабочим потоком по выделению.\r\nUSN проверяется раз в 3 секунды; при отказе доступа проверка прекращается до обновления.\r\nФоновая анимация и захват экрана не используются.",
            0,
            PERF_HELP,
        );
        let sections = child(hwnd, "LISTBOX", "", WS_TABSTOP | 1 | 0x100, SECTIONS);
        for label in [
            "Внешний вид",
            "Поиск",
            "Индексация",
            "Хранилище",
            "Горячие клавиши",
            "Производительность",
            "О программе",
        ] {
            SendMessageW(sections, 0x180, 0, wide(label).as_ptr() as isize);
        }
        SendMessageW(sections, 0x186, 2, 0);
        let extensions = child(hwnd, "EDIT", "", WS_TABSTOP | 0x80, EXTENSION);
        SendMessageW(extensions, 0x1501, 0, wide("exe,dll…").as_ptr() as isize);
        let categories = child(hwnd, "COMBOBOX", "", WS_TABSTOP | 3, CATEGORY);
        for label in [
            "Все типы",
            "Документы",
            "Изображения",
            "Видео",
            "Аудио",
            "Архивы",
            "Программы",
            "Папки",
        ] {
            SendMessageW(categories, 0x143, 0, wide(label).as_ptr() as isize);
        }
        SendMessageW(categories, 0x14e, 0, 0);
        child(hwnd, "BUTTON", "По релевантности", WS_TABSTOP | 3, RANK);
        child(hwnd, "STATIC", "Каталог:", 0, 201);
        let root = "*".to_string();
        child(hwnd, "EDIT", &root, WS_TABSTOP | 0x80, ROOT);
        child(hwnd, "BUTTON", "Индексировать", WS_TABSTOP, BUILD);
        child(hwnd, "BUTTON", "Все диски", WS_TABSTOP, ALL);
        child(hwnd, "BUTTON", "Обновить", WS_TABSTOP, LOAD);
        child(hwnd, "BUTTON", "←", WS_TABSTOP, BACK);
        child(hwnd, "BUTTON", "→", WS_TABSTOP, FORWARD);
        child(hwnd, "BUTTON", "Вид", WS_TABSTOP, VIEW);
        child(hwnd, "BUTTON", "Закрепить", WS_TABSTOP, PIN);
        child(hwnd, "BUTTON", "Быстрый поиск", WS_TABSTOP, SPOTLIGHT);
        child(hwnd, "BUTTON", "Копировать", WS_TABSTOP, COPY);
        child(
            hwnd,
            "BUTTON",
            "Копировать запрос для ИИ",
            WS_TABSTOP,
            COPY_PROMPT,
        );
        child(
            hwnd,
            "STATIC",
            "Выберите объект для просмотра метаданных. Содержимое файла не читается.",
            0,
            DETAILS,
        );
        child(
            hwnd,
            "STATIC",
            "Глобальная клавиша: off (отключена). Пример: Ctrl+Alt+Space",
            0,
            HOTKEY_LABEL,
        );
        let shortcut = std::fs::read_to_string(index_path().with_extension("hotkey"))
            .unwrap_or_else(|_| "off".into());
        child(hwnd, "EDIT", &shortcut, WS_TABSTOP | 0x80, HOTKEY);
        child(
            hwnd,
            "BUTTON",
            "Применить сочетание",
            WS_TABSTOP,
            HOTKEY_SAVE,
        );
        child(hwnd, "BUTTON", "↑", WS_TABSTOP, UP);
        child(hwnd, "EDIT", "Этот компьютер", WS_TABSTOP | 0x80, ADDRESS);
        child(hwnd, "BUTTON", "→", WS_TABSTOP, GO);
        child(hwnd, "STATIC", "Навигация", 0, 205);
        let sidebar_control = child(
            hwnd,
            "LISTBOX",
            "",
            WS_TABSTOP | WS_VSCROLL | 0x1 | 0x100,
            SIDEBAR,
        );
        SendMessageW(
            sidebar_control,
            0x180,
            0,
            wide("Этот компьютер").as_ptr() as isize,
        );
        let mut sidebar = vec![None];
        SendMessageW(
            sidebar_control,
            0x180,
            0,
            wide("Недавние файлы").as_ptr() as isize,
        );
        sidebar.push(Some("hypersearch:recent".into()));
        for root in filesystem::drives() {
            SendMessageW(
                sidebar_control,
                0x180,
                0,
                wide(&format!("  Диск {root}")).as_ptr() as isize,
            );
            sidebar.push(Some(root));
        }
        SendMessageW(sidebar_control, 0x186, 0, 0);
        let mut favorites = Vec::new();
        for id in [
            windows_sys::Win32::UI::Shell::FOLDERID_Profile,
            windows_sys::Win32::UI::Shell::FOLDERID_Desktop,
            windows_sys::Win32::UI::Shell::FOLDERID_Documents,
            windows_sys::Win32::UI::Shell::FOLDERID_Downloads,
            windows_sys::Win32::UI::Shell::FOLDERID_Pictures,
        ] {
            if let Some(path) = super::folders::known(&id) {
                favorites.push(path.to_string_lossy().into_owned());
            }
        }
        favorites.extend(
            std::fs::read_to_string(index_path().with_extension("pins"))
                .unwrap_or_default()
                .lines()
                .map(str::to_string),
        );
        for path in favorites {
            if !sidebar.contains(&Some(path.clone())) {
                SendMessageW(sidebar_control, 0x180, 0, wide(&path).as_ptr() as isize);
                sidebar.push(Some(path));
            }
        }
        child(hwnd, "BUTTON", "Открыть", WS_TABSTOP, OPEN);
        child(hwnd, "BUTTON", "Расположение", WS_TABSTOP, LOCATION);
        child(hwnd, "BUTTON", "Параметры", WS_TABSTOP, SETTINGS);
        child(hwnd, "STATIC", "Хранилище → Анализ диска", 0, WDS_LABEL);
        child(hwnd, "BUTTON", "Скачать WinDirStat", WS_TABSTOP, WDS);
        child(hwnd, "BUTTON", "Официальный сайт", WS_TABSTOP, WDS_SITE);
        child(
            hwnd,
            "BUTTON",
            "Проверить обновления",
            WS_TABSTOP,
            WDS_UPDATE,
        );
        child(hwnd, "EDIT", "", WS_TABSTOP | 0x80, WDS_PATH);
        child(hwnd, "BUTTON", "Запомнить путь", WS_TABSTOP, WDS_SAVE);
        child(hwnd, "BUTTON", "Экспорт структуры", WS_TABSTOP, EXPORT);
        child(hwnd, "BUTTON", "Отчёт для ИИ", WS_TABSTOP, AI_REPORT);
        child(hwnd, "BUTTON", "Отмена", WS_TABSTOP, CANCEL_EXPORT);
        child(
            hwnd,
            "STATIC",
            "Область (пусто = текущая папка), формат; ниже глубина и мин. байт.\r\nИсключения — абсолютные пути через ;",
            0,
            EXPORT_HELP,
        );
        let export_roots = child(hwnd, "COMBOBOX", "", WS_TABSTOP | 2, EXPORT_ROOT);
        for drive in filesystem::drives() {
            SendMessageW(export_roots, 0x143, 0, wide(&drive).as_ptr() as isize);
        }
        child(
            hwnd,
            "EDIT",
            "1000",
            WS_TABSTOP | 0x80 | 0x2000,
            EXPORT_DEPTH,
        );
        child(hwnd, "EDIT", "0", WS_TABSTOP | 0x80 | 0x2000, EXPORT_MIN);
        child(
            hwnd,
            "BUTTON",
            "Нейтральные имена",
            WS_TABSTOP | 3,
            EXPORT_NEUTRAL,
        );
        child(
            hwnd,
            "BUTTON",
            "Компактный отчёт",
            WS_TABSTOP | 3,
            EXPORT_COMPACT,
        );
        let exclude = child(hwnd, "EDIT", "", WS_TABSTOP | 0x80, EXPORT_EXCLUDE);
        SendMessageW(
            exclude,
            0x1501,
            0,
            wide("Исключить: C:\\личное;D:\\работа").as_ptr() as isize,
        );
        let formats = child(hwnd, "COMBOBOX", "", WS_TABSTOP | 3, EXPORT_FORMAT);
        for name in ["Markdown", "JSON", "CSV"] {
            SendMessageW(formats, 0x143, 0, wide(name).as_ptr() as isize);
        }
        SendMessageW(formats, 0x14e, 0, 0);
        let privacy = child(
            hwnd,
            "BUTTON",
            "Скрыть имя пользователя",
            WS_TABSTOP | 3,
            SAFE_EXPORT,
        );
        SendMessageW(privacy, 0xf1, 1, 0);
        child(hwnd, "STATIC", "Поиск:", 0, 202);
        let query = child(hwnd, "EDIT", "", WS_TABSTOP | 0x80, QUERY);
        SendMessageW(
            query,
            0x1501,
            0,
            wide("Поиск · ext:exe type:apps rank:on").as_ptr() as isize,
        );
        let combo = child(hwnd, "COMBOBOX", "", WS_TABSTOP | 0x3, MODE);
        for name in ["Часть имени", "Начало имени", "Точное имя"] {
            SendMessageW(combo, 0x143, 0, wide(name).as_ptr() as isize);
        }
        SendMessageW(combo, 0x14e, 0, 0);
        child(hwnd, "STATIC", "Память:", 0, 203);
        let profile = child(hwnd, "COMBOBOX", "", WS_TABSTOP | 0x3, PROFILE);
        for name in [
            "Экономный",
            "Сбалансированный",
            "Быстрый (тестирование)",
            "Свой бюджет RAM",
        ] {
            SendMessageW(profile, 0x143, 0, wide(name).as_ptr() as isize);
        }
        SendMessageW(profile, 0x14e, 2, 0);
        child(hwnd, "STATIC", "Бюджет, МБ:", 0, 204);
        child(hwnd, "EDIT", "384", WS_TABSTOP | 0x80 | 0x2000, BUDGET);
        child(hwnd, "BUTTON", "Оценить RAM", WS_TABSTOP, ANALYZE);
        child(hwnd, "BUTTON", "Применить и загрузить", WS_TABSTOP, APPLY);
        child(
            hwnd,
            "STATIC",
            "Первый запуск: сначала оценим память. Полный поисковый индекс не загружается до вашего выбора.",
            0,
            PLAN,
        );
        let output = child(
            hwnd,
            "SysListView32",
            "",
            WS_TABSTOP | LVS_REPORT | LVS_OWNERDATA | LVS_SHOWSELALWAYS | LVS_SHAREIMAGELISTS,
            OUTPUT,
        );
        SendMessageW(
            output,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
        );
        SetWindowTheme(output, wide("Explorer").as_ptr(), null());
        SetWindowTheme(sidebar_control, wide("Explorer").as_ptr(), null());
        for (i, (title, width)) in [("Имя", 270), ("Расположение", 520), ("Тип", 130)]
            .into_iter()
            .enumerate()
        {
            let mut name = wide(title);
            let column = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH | LVCF_SUBITEM,
                cx: width,
                pszText: name.as_mut_ptr(),
                iSubItem: i as i32,
                ..std::mem::zeroed()
            };
            SendMessageW(output, LVM_INSERTCOLUMNW, i, &column as *const _ as isize);
        }
        child(hwnd, "STATIC", "Загрузка индекса…", 0, STATUS);
        child(hwnd, "BUTTON", "Ещё 100", WS_TABSTOP, NEXT);
        let (tasks, rx) = mpsc::channel();
        let (tx, events) = mpsc::channel();
        let generation = Arc::new(AtomicU64::new(0));
        let mut app = App {
            smoke: if smoke {
                Some(Smoke {
                    start: smoke_start,
                    query: Instant::now(),
                    phase: 0,
                    log: vec![],
                    idle_cpu: 0.,
                })
            } else {
                None
            },
            hwnd,
            tasks,
            events,
            generation: generation.clone(),
            next: None,
            ready: false,
            rows: Vec::new(),
            scope: None,
            history: Vec::new(),
            forward: Vec::new(),
            setup: true,
            sidebar,
            status: String::new(),
            plan: None,
            choice: None,
            saved: super::settings::Preference::load(&index_path().with_extension("preferences")),
            plan_saved: false,
            plan_root: "*".into(),
            export_cancel: None,
            download_cancel: None,
            closing: false,
            icons: std::collections::HashMap::new(),
            object_count: 0,
            last_search: "ещё не выполнялся".into(),
        };
        app.refresh_windirstat();
        if let Some(p) = app.saved
            && p.budget_mb.is_none()
            && index_path().is_file()
        {
            app.saved = None;
            app.choice = Some(p.mode);
            app.plan_saved = true;
            SendMessageW(
                profile,
                0x14e,
                match p.mode {
                    MemoryMode::Economy => 0,
                    MemoryMode::Balanced => 1,
                    MemoryMode::Fast => 2,
                },
                0,
            );
            app.busy("Загрузка сохранённых таблиц поиска…");
            let _ = app.tasks.send(Task::Load(p.mode));
        } else {
            app.busy("Подсчёт объёма памяти без загрузки поискового индекса…");
            let _ = app.tasks.send(Task::Analyze("*".into()));
        }
        let settings_window = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("HyperSearch — Параметры").as_ptr(),
            WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1040 * initial_dpi / 96,
            620 * initial_dpi / 96,
            hwnd,
            std::ptr::null_mut(),
            instance,
            null(),
        );
        SetPropW(hwnd, wide("HyperSearch.Settings").as_ptr(), settings_window);
        for id in [
            201,
            ROOT,
            BUILD,
            ALL,
            203,
            PROFILE,
            204,
            BUDGET,
            ANALYZE,
            APPLY,
            PLAN,
            WDS,
            WDS_PATH,
            WDS_SAVE,
            WDS_SITE,
            WDS_UPDATE,
            WDS_LABEL,
            THEME,
            GLASS,
            EXPORT,
            AI_REPORT,
            CANCEL_EXPORT,
            EXPORT_FORMAT,
            SAFE_EXPORT,
            HOTKEY,
            HOTKEY_SAVE,
            HOTKEY_LABEL,
            COPY_PROMPT,
            SECTIONS,
            ABOUT,
            SEARCH_HELP,
            PERF_HELP,
            ANIMATIONS,
            EXPORT_ROOT,
            EXPORT_DEPTH,
            EXPORT_MIN,
            EXPORT_NEUTRAL,
            EXPORT_EXCLUDE,
            EXPORT_COMPACT,
            EXPORT_HELP,
        ] {
            SetParent(GetDlgItem(hwnd, id as i32), settings_window);
        }
        let popup = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class.as_ptr(),
            wide("HyperSearch — Spotlight").as_ptr(),
            WS_POPUP | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME,
            0,
            0,
            760,
            430,
            hwnd,
            std::ptr::null_mut(),
            instance,
            null(),
        );
        SetPropW(hwnd, wide("HyperSearch.Palette").as_ptr(), popup);
        child(popup, "EDIT", "", WS_TABSTOP | 0x80, QUERY);
        let popup_list = child(
            popup,
            "SysListView32",
            "",
            WS_TABSTOP
                | LVS_REPORT
                | LVS_OWNERDATA
                | LVS_SINGLESEL
                | LVS_SHOWSELALWAYS
                | LVS_SHAREIMAGELISTS,
            OUTPUT,
        );
        for (i, title) in ["Имя", "Расположение"].into_iter().enumerate() {
            let mut title = wide(title);
            let column = LVCOLUMNW {
                mask: LVCF_TEXT | LVCF_WIDTH,
                cx: if i == 0 { 260 } else { 430 },
                pszText: title.as_mut_ptr(),
                ..std::mem::zeroed()
            };
            SendMessageW(
                popup_list,
                LVM_INSERTCOLUMNW,
                i,
                &column as *const _ as isize,
            );
        }
        SendMessageW(
            popup_list,
            LVM_SETEXTENDEDLISTVIEWSTYLE,
            0,
            (LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER) as isize,
        );
        let look = super::appearance::Appearance::load(&index_path().with_extension("appearance"));
        LOOK.with(|value| value.set(look));
        set(control(hwnd, THEME), &format!("Тема: {:?}", look.theme));
        set(
            control(hwnd, GLASS),
            if look.transparency {
                "Прозрачность: вкл"
            } else {
                "Прозрачность: выкл"
            },
        );
        set(
            control(hwnd, ANIMATIONS),
            if look.animations {
                "Анимации: вкл"
            } else {
                "Анимации: выкл"
            },
        );
        look.apply(settings_window);
        look.apply(hwnd);
        look.apply(popup);
        if look.transparency {
            let acrylic = 3u32;
            windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                popup,
                38,
                (&acrylic as *const u32).cast(),
                4,
            );
        }
        if let Err(e) = register_shortcut(hwnd, &shortcut) {
            set(control(hwnd, HOTKEY_LABEL), &e);
        }
        APP.with(|a| *a.borrow_mut() = Some(app));
        let window = hwnd as usize;
        std::thread::spawn(move || worker(rx, tx, generation, window));
        SetTimer(hwnd, 1, 3000, None);
        layout(hwnd, true);
        if !smoke {
            ShowWindow(hwnd, SW_SHOW);
        } else {
            SetTimer(hwnd, 2, 30000, None);
        }
        SetFocus(control(hwnd, QUERY));
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            if message.message == WM_KEYDOWN {
                use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
                let ctrl = GetKeyState(0x11) < 0;
                if ctrl
                    && message.wParam == 0x43
                    && (GetFocus() == control(hwnd, OUTPUT) || GetFocus() == popup_list)
                {
                    SendMessageW(hwnd, WM_COMMAND, COPY, 0);
                    continue;
                }
                if ctrl && message.wParam == 0x4c {
                    SetFocus(control(hwnd, ADDRESS));
                    SendMessageW(control(hwnd, ADDRESS), EM_SETSEL, 0, -1);
                    continue;
                }
                if ctrl && message.wParam == 0x46 {
                    SetFocus(control(hwnd, QUERY));
                    continue;
                }
                if GetKeyState(0x12) < 0 && message.wParam == 0x25 {
                    SendMessageW(hwnd, WM_COMMAND, BACK, 0);
                    continue;
                }
                if GetKeyState(0x12) < 0 && message.wParam == 0x27 {
                    SendMessageW(hwnd, WM_COMMAND, FORWARD, 0);
                    continue;
                }
            }
            if message.message == WM_KEYDOWN && IsWindowVisible(popup) != 0 {
                if message.wParam == 0x1b {
                    ShowWindow(popup, SW_HIDE);
                    continue;
                }
                let focus = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus();
                if message.wParam == 0x28 && focus == GetDlgItem(popup, QUERY as i32) {
                    SetFocus(popup_list);
                    let state = LVITEMW {
                        stateMask: LVIS_SELECTED | LVIS_FOCUSED,
                        state: LVIS_SELECTED | LVIS_FOCUSED,
                        ..std::mem::zeroed()
                    };
                    SendMessageW(popup_list, LVM_SETITEMSTATE, 0, &state as *const _ as isize);
                    continue;
                }
                if message.wParam == 0x0d {
                    APP.with(|a| {
                        if let Some(app) = a.borrow_mut().as_mut() {
                            app.open(false);
                        }
                    });
                    ShowWindow(popup, SW_HIDE);
                    continue;
                }
            }
            if message.message == WM_KEYDOWN && message.wParam == 0x0d {
                let focus = windows_sys::Win32::UI::Input::KeyboardAndMouse::GetFocus();
                if focus == control(hwnd, ADDRESS) {
                    APP.with(|a| {
                        if let Some(app) = a.borrow_mut().as_mut() {
                            let p = text(focus);
                            app.navigate(
                                if p.trim() == "*" || p == "Этот компьютер" || p.trim().is_empty()
                                {
                                    None
                                } else {
                                    Some(p)
                                },
                                true,
                            );
                        }
                    });
                    continue;
                }
                if focus == control(hwnd, OUTPUT) {
                    APP.with(|a| {
                        if let Some(app) = a.borrow_mut().as_mut() {
                            app.open(false);
                        }
                    });
                    continue;
                }
            }
            let active = if IsWindowVisible(popup) != 0 {
                popup
            } else if IsWindowVisible(settings_window) != 0
                && GetForegroundWindow() == settings_window
            {
                settings_window
            } else {
                hwnd
            };
            if IsDialogMessageW(active, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        APP.with(|a| {
            a.borrow_mut().take();
        });
        windows_sys::Win32::UI::Input::KeyboardAndMouse::UnregisterHotKey(hwnd, 1);
        if smoke && message.wParam != 0 {
            std::process::exit(1);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn spotlight_bounds_across_dpi_and_negative_monitors() {
        for work in [
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            RECT {
                left: -1920,
                top: -400,
                right: 0,
                bottom: 680,
            },
            RECT {
                left: 1920,
                top: 0,
                right: 4480,
                bottom: 1400,
            },
            RECT {
                left: 0,
                top: 0,
                right: 640,
                bottom: 480,
            },
        ] {
            for dpi in [96, 120, 144, 192] {
                let bounds = palette_bounds(&work, dpi);
                assert!(
                    bounds.left >= work.left
                        && bounds.top >= work.top
                        && bounds.right <= work.right
                        && bounds.bottom <= work.bottom
                );
                assert_eq!(
                    bounds.right - bounds.left,
                    (760 * dpi as i32 / 96).min(work.right - work.left)
                );
            }
        }
    }
    #[test]
    fn native_details_control_renders_unicode_and_selection() {
        unsafe {
            let common = INITCOMMONCONTROLSEX {
                dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
                dwICC: ICC_LISTVIEW_CLASSES,
            };
            assert_ne!(InitCommonControlsEx(&common), 0);
            let parent = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("test").as_ptr(),
                0,
                0,
                0,
                800,
                500,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            assert!(!parent.is_null());
            let list = child(parent, "SysListView32", "", LVS_REPORT, OUTPUT);
            assert!(!list.is_null());
            for i in 0..3 {
                let col = LVCOLUMNW {
                    mask: LVCF_WIDTH,
                    cx: 200,
                    ..std::mem::zeroed()
                };
                assert_eq!(
                    SendMessageW(list, LVM_INSERTCOLUMNW, i, &col as *const _ as isize),
                    i as isize
                );
            }
            let (tasks, requests) = mpsc::channel();
            let (_, events) = mpsc::channel();
            let mut app = App {
                smoke: None,
                hwnd: parent,
                tasks,
                events,
                generation: Arc::new(AtomicU64::new(0)),
                next: None,
                ready: true,
                rows: vec![Row {
                    name: "Отчёт.xlsx".into(),
                    path: "C:\\Docs\\Отчёт.xlsx".into(),
                    kind: "Файл XLSX".into(),
                    directory: false,
                }],
                scope: None,
                history: vec![],
                forward: vec![],
                setup: false,
                sidebar: vec![None],
                status: String::new(),
                plan: None,
                choice: Some(MemoryMode::Fast),
                saved: None,
                plan_saved: false,
                plan_root: "*".into(),
                export_cancel: None,
                download_cancel: None,
                closing: false,
                icons: std::collections::HashMap::new(),
                object_count: 0,
                last_search: String::new(),
            };
            app.render_rows();
            assert_eq!(SendMessageW(list, LVM_GETITEMCOUNT, 0, 0), 1);
            let mut buffer = [0u16; 256];
            let mut item: LVITEMW = std::mem::zeroed();
            item.iSubItem = 0;
            item.pszText = buffer.as_mut_ptr();
            item.cchTextMax = 256;
            let n = SendMessageW(list, LVM_GETITEMTEXTW, 0, &mut item as *mut _ as isize);
            assert_eq!(
                String::from_utf16_lossy(&buffer[..n as usize]),
                "Отчёт.xlsx"
            );
            let selection = LVITEMW {
                stateMask: LVIS_SELECTED,
                state: LVIS_SELECTED,
                ..std::mem::zeroed()
            };
            SendMessageW(list, LVM_SETITEMSTATE, 0, &selection as *const _ as isize);
            assert_eq!(app.selected().unwrap().name, "Отчёт.xlsx");
            child(parent, "EDIT", "", 0, ADDRESS);
            child(parent, "EDIT", "", 0, QUERY);
            child(parent, "STATIC", "", 0, STATUS);
            child(parent, "COMBOBOX", "", 0, MODE);
            app.navigate(Some("C:\\Docs".into()), true);
            assert_eq!(app.history, vec![None]);
            assert_eq!(app.scope.as_deref(), Some("C:\\Docs"));
            match requests.try_recv().unwrap() {
                Task::Search { scope, text, .. } => {
                    assert_eq!(scope.as_deref(), Some("C:\\Docs"));
                    assert!(text.is_empty());
                }
                _ => panic!("expected navigation request"),
            }
            DestroyWindow(list);
            let virtual_list = child(
                parent,
                "SysListView32",
                "",
                LVS_REPORT | LVS_OWNERDATA,
                OUTPUT,
            );
            app.rows = vec![Row {
                name: "Очень длинное имя — 文件.xlsx".into(),
                path: "C:\\Docs\\文件.xlsx".into(),
                kind: "XLSX".into(),
                directory: false,
            }];
            app.render_rows();
            assert_eq!(SendMessageW(virtual_list, LVM_GETITEMCOUNT, 0, 0), 1);
            APP.with(|cell| *cell.borrow_mut() = Some(app));
            let mut info: NMLVDISPINFOW = std::mem::zeroed();
            let mut buffer = [0u16; 128];
            info.hdr.idFrom = OUTPUT;
            info.hdr.code = LVN_GETDISPINFOW;
            info.item.mask = LVIF_TEXT;
            info.item.pszText = buffer.as_mut_ptr();
            info.item.cchTextMax = buffer.len() as i32;
            window_proc(parent, WM_NOTIFY, OUTPUT, &mut info as *mut _ as isize);
            let len = buffer.iter().position(|&c| c == 0).unwrap();
            assert_eq!(
                String::from_utf16_lossy(&buffer[..len]),
                "Очень длинное имя — 文件.xlsx"
            );
            info.item.cchTextMax = 4;
            buffer.fill(0xffff);
            window_proc(parent, WM_NOTIFY, OUTPUT, &mut info as *mut _ as isize);
            assert_eq!(buffer[3], 0);
            assert_eq!(buffer[4], 0xffff);
            let mut app = APP.with(|cell| cell.borrow_mut().take().unwrap());
            let second = CreateWindowExW(
                0,
                wide("STATIC").as_ptr(),
                wide("hotkey-test").as_ptr(),
                0,
                0,
                0,
                100,
                100,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                GetModuleHandleW(null()),
                null(),
            );
            let free = (0..10)
                .map(|n| format!("Ctrl+Alt+Shift+{n}"))
                .find(|key| register_shortcut(parent, key).is_ok())
                .expect("no free hotkey among test candidates");
            assert!(register_shortcut(second, &free).is_err());
            assert!(register_shortcut(parent, "invalid shortcut").is_err());
            assert!(
                register_shortcut(second, &free).is_err(),
                "invalid input must preserve previous registration"
            );
            register_shortcut(parent, "off").unwrap();
            register_shortcut(second, &free).unwrap();
            register_shortcut(second, "off").unwrap();
            DestroyWindow(second);
            let download = Arc::new(AtomicBool::new(false));
            let export = Arc::new(AtomicBool::new(false));
            app.download_cancel = Some(download.clone());
            app.export_cancel = Some(export.clone());
            let (done, events) = mpsc::channel();
            app.events = events;
            APP.with(|cell| *cell.borrow_mut() = Some(app));
            window_proc(parent, WM_CLOSE, 0, 0);
            assert!(download.load(Ordering::Relaxed) && export.load(Ordering::Relaxed));
            assert_ne!(
                IsWindow(parent),
                0,
                "close waits for cancellation acknowledgements"
            );
            done.send(Event::Downloaded(Ok(
                super::super::windirstat_download::Package {
                    path: PathBuf::from("C:\\never-executed.msi"),
                    version: "test".into(),
                    sha256: "0".repeat(64),
                    cached: true,
                },
            )))
            .unwrap();
            APP.with(|cell| cell.borrow_mut().as_mut().unwrap().events());
            assert_ne!(
                IsWindow(parent),
                0,
                "export still pending; no install confirmation on close"
            );
            done.send(Event::ExportDone(Box::new(Err("cancelled".into()))))
                .unwrap();
            APP.with(|cell| cell.borrow_mut().as_mut().unwrap().events());
            assert_eq!(IsWindow(parent), 0);
            APP.with(|cell| {
                cell.borrow_mut().take();
            });
        }
    }
}
