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
enum Task {
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
    Freshness(String),
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
fn worker(rx: Receiver<Task>, tx: Sender<Event>, generation: Arc<AtomicU64>, window: usize) {
    let (metadata_tx, metadata_rx) = mpsc::channel::<String>();
    let metadata_events = tx.clone();
    std::thread::spawn(move || {
        for mut path in &metadata_rx {
            while let Ok(newer) = metadata_rx.try_recv() {
                path = newer;
            }
            let result = match std::fs::symlink_metadata(&path) {
                Ok(m) => format!(
                    "{}\r\nЛогический размер: {} байт\r\nИзменён: {:?}\r\nСоздан: {:?}\r\nТолько чтение: {}\r\nФизический размер: не измерен",
                    path,
                    m.len(),
                    m.modified().ok(),
                    m.created().ok(),
                    m.permissions().readonly()
                ),
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
                        Event::Freshness(format!(
                            "Фон USN: применено {count} записей без перестроения поисковых таблиц"
                        ))
                    }
                    Err(e) => {
                        monitor_enabled = false;
                        Event::Freshness(format!(
                            "Актуальность не подтверждена: {e}. Сохранённый индекс доступен."
                        ))
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
                        &std::collections::HashMap::new(),
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
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        GetMonitorInfoW(monitor, &mut info);
        let width = 760.min(info.rcWork.right - info.rcWork.left);
        let height = 430.min(info.rcWork.bottom - info.rcWork.top);
        SetWindowPos(
            p,
            HWND_TOP,
            info.rcWork.left + (info.rcWork.right - info.rcWork.left - width) / 2,
            info.rcWork.top + (info.rcWork.bottom - info.rcWork.top - height) / 3,
            width,
            height,
            SWP_SHOWWINDOW,
        );
        SetForegroundWindow(p);
        SetFocus(GetDlgItem(p, QUERY as i32));
    }
}
fn register_shortcut(hwnd: HWND, value: &str) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::*;
    unsafe {
        UnregisterHotKey(hwnd, 1);
    }
    if value.trim().is_empty() || value.eq_ignore_ascii_case("off") {
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
    if unsafe { RegisterHotKey(hwnd, 1, modifiers, key) } == 0 {
        return Err("Сочетание занято системой или другой программой".into());
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
    fn export(&mut self, compact: bool) {
        use hypersearch_engine::storage_analysis::{self, Format, Options};
        if !self.ready || self.export_cancel.is_some() {
            return;
        }
        let source = self
            .scope
            .clone()
            .unwrap_or_else(|| text(control(self.hwnd, ROOT)));
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
        options.compact = compact;
        options.safe = unsafe { SendMessageW(control(self.hwnd, SAFE_EXPORT), 0xf0, 0, 0) } != 0;
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
        if let Some(path) = super::windirstat::installed() {
            set(control(self.hwnd, WDS), "Открыть WinDirStat");
            set(control(self.hwnd, WDS_PATH), &path.to_string_lossy());
            set(
                control(self.hwnd, WDS_LABEL),
                "Хранилище → Анализ диска: WinDirStat найден. Путь сохранён.",
            );
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
                Event::Freshness(status) => {
                    self.status = status.clone();
                    set(control(self.hwnd, STATUS), &status);
                }
                Event::Metadata(path, result) => {
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
                        &format!("HyperSearch beta 0.1 — {count} объектов"),
                    );
                    set(
                        control(self.hwnd, ADDRESS),
                        self.scope.as_deref().unwrap_or("Этот компьютер"),
                    );
                    self.search(None);
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
                    self.next = next;
                    self.render_rows();
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
        SendMessageW(h, WM_SETFONT, GetStockObject(DEFAULT_GUI_FONT) as usize, 1);
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
        let w = r.right.max(1000);
        let h = r.bottom.max(580);
        for id in [
            201, ROOT, BUILD, ALL, 203, PROFILE, 204, BUDGET, ANALYZE, APPLY, PLAN,
        ] {
            ShowWindow(control(hwnd, id), if setup { SW_SHOW } else { SW_HIDE });
        }
        ShowWindow(control(hwnd, 202), SW_HIDE);
        let settings = GetPropW(hwnd, wide("HyperSearch.Settings").as_ptr());
        if !settings.is_null() {
            ShowWindow(settings, if setup { SW_SHOW } else { SW_HIDE });
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
            MoveWindow(control(hwnd, id), x, y, cw, ch, 1);
        }
        let section = SendMessageW(control(hwnd, SECTIONS), 0x188, 0, 0);
        if !settings.is_null() {
            let mut r: RECT = std::mem::zeroed();
            GetClientRect(settings, &mut r);
            let sw = r.right.max(1000);
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
                MoveWindow(control(hwnd, id), x, y, width, height, 1);
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
                    MoveWindow(
                        GetDlgItem(hwnd, QUERY as i32),
                        20,
                        18,
                        (r.right - 40).max(100),
                        38,
                        1,
                    );
                    MoveWindow(
                        GetDlgItem(hwnd, OUTPUT as i32),
                        20,
                        72,
                        (r.right - 40).max(100),
                        (r.bottom - 92).max(60),
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
                SetBkColor(w as HDC, bg);
                let brush = GetStockObject(DC_BRUSH);
                SetDCBrushColor(w as HDC, bg);
                return brush as isize;
            }
        }
        WM_ERASEBKGND => {
            let look = LOOK.with(|look| look.get());
            let bg = if look.dark() { 0x00282020 } else { 0x00faf7f4 };
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
                    a.scope = None;
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
                    .unwrap_or(true)
            });
            layout(hwnd, setup);
            return 0;
        }
        WM_GETMINMAXINFO => {
            if l != 0 {
                let info = unsafe { &mut *(l as *mut MINMAXINFO) };
                info.ptMinTrackSize = POINT { x: 1040, y: 660 };
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
                    });
                }
                if notify.idFrom == OUTPUT && notify.code == LVN_COLUMNCLICK {
                    let column = unsafe { (*(l as *const NMLISTVIEW)).iSubItem };
                    APP.with(|a| {
                        if let Ok(mut a) = a.try_borrow_mut()
                            && let Some(a) = a.as_mut()
                        {
                            a.rows.sort_by_cached_key(|r| match column {
                                1 => r.path.to_lowercase(),
                                2 => r.kind.to_lowercase(),
                                _ => r.name.to_lowercase(),
                            });
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
                let id = w & 0xffff;
                let event = (w >> 16) & 0xffff;
                match (id, event) {
                    (SECTIONS, 1) => layout(hwnd, true),
                    (EXTENSION, 0x300) | (CATEGORY, 1) | (RANK, 0) => app.search(None),
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
                        app.scope = None;
                        show_palette(hwnd);
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
                        }
                        layout(hwnd, app.setup);
                    }
                    (WDS, 0) => {
                        let target = super::windirstat::installed()
                            .map(|p| p.to_string_lossy().into_owned())
                            .unwrap_or_else(|| super::windirstat::RELEASES.into());
                        unsafe {
                            ShellExecuteW(
                                hwnd,
                                wide("open").as_ptr(),
                                wide(&target).as_ptr(),
                                null(),
                                null(),
                                SW_SHOWNORMAL,
                            );
                        }
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
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("HyperSearch beta 0.1").as_ptr(),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            1200,
            800,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        super::appearance::Appearance::load(&index_path().with_extension("appearance")).apply(hwnd);
        child(hwnd, "BUTTON", "Тема: System", WS_TABSTOP, THEME);
        child(hwnd, "BUTTON", "Прозрачность", WS_TABSTOP, GLASS);
        child(hwnd, "BUTTON", "Анимации: выкл", WS_TABSTOP, ANIMATIONS);
        child(
            hwnd,
            "STATIC",
            "HyperSearch 0.2\r\nНативный поиск файлов Windows / Rust\r\nИндекс хранится локально. Отчёты не отправляются в сеть.\r\nWinDirStat — отдельная программа со своей лицензией.\r\nhttps://github.com/Noki1945/HyperSearch",
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
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            for name in ["Desktop", "Documents", "Downloads", "Pictures"] {
                let path = PathBuf::from(&profile).join(name);
                if path.is_dir() {
                    favorites.push(path.to_string_lossy().into_owned());
                }
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
            WS_TABSTOP | LVS_REPORT | LVS_OWNERDATA | LVS_SHOWSELALWAYS,
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
            1040,
            620,
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
            WS_TABSTOP | LVS_REPORT | LVS_OWNERDATA | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
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
        look.apply(settings_window);
        look.apply(popup);
        if let Err(e) = register_shortcut(hwnd, &shortcut) {
            set(control(hwnd, HOTKEY_LABEL), &e);
        }
        APP.with(|a| *a.borrow_mut() = Some(app));
        let window = hwnd as usize;
        std::thread::spawn(move || worker(rx, tx, generation, window));
        SetTimer(hwnd, 1, 3000, None);
        layout(hwnd, true);
        ShowWindow(hwnd, SW_SHOW);
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
    }
}
#[cfg(test)]
mod tests {
    use super::*;
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
            DestroyWindow(parent);
        }
    }
}
