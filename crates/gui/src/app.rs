use super::view_model::{self, Row};
use hypersearch_engine::{
    filesystem, indexer, monitor,
    planning::{self, Summary},
    search::{MemoryMode, Mode, Query, SearchIndex},
    storage,
};
use std::{
    cell::RefCell,
    path::PathBuf,
    ptr::null,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
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
enum Task {
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
    setup: bool,
    sidebar: Vec<Option<String>>,
    status: String,
    plan: Option<Summary>,
    choice: Option<MemoryMode>,
    saved: Option<super::settings::Preference>,
    plan_saved: bool,
    plan_root: String,
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
    let mut index: Option<SearchIndex> = None;
    // One most recent page is enough to make immediate repeated queries allocation-light.
    let mut cache: Option<CachedPage> = None;
    for task in rx {
        let event = match task {
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
                        index = Some(built);
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
                let page = if text.trim().is_empty() {
                    let Some(page) = index.children(scope.as_deref().unwrap(), after, 100) else {
                        let _=tx.send(Event::Results{generation:version,rows:vec![],next:None,status:"Этой папки нет в загруженном индексе. Измените область или выполните индексацию.".into()});
                        unsafe {
                            PostMessageW(window as HWND, DONE, 0, 0);
                        }
                        continue;
                    };
                    page
                } else {
                    index.search_cancellable(
                        &Query {
                            text: &text,
                            mode,
                            extension: None,
                            path: scope.as_deref(),
                        },
                        after,
                        100,
                        || generation.load(Ordering::Relaxed) != version,
                    )
                };
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
    unsafe { GetDlgItem(hwnd, id as i32) }
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
    fn render_rows(&self) {
        unsafe {
            let list = control(self.hwnd, OUTPUT);
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
        let i = unsafe {
            SendMessageW(
                control(self.hwnd, OUTPUT),
                LVM_GETNEXTITEM,
                usize::MAX,
                LVNI_SELECTED as isize,
            )
        };
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
        let query = text(control(self.hwnd, QUERY));
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
        let list_top = if setup { 344 } else { 96 };
        for (id, x, y, cw, ch) in [
            (BACK, 12, 12, 34, 30),
            (UP, 52, 12, 34, 30),
            (LOAD, 92, 12, 100, 30),
            (ADDRESS, 204, 12, w - 554, 30),
            (GO, w - 342, 12, 50, 30),
            (QUERY, w - 280, 12, 268, 30),
            (SIDEBAR, 12, 96, 178, h - 196),
            (205, 12, 62, 178, 24),
            (OPEN, 204, 54, 96, 30),
            (LOCATION, 308, 54, 180, 30),
            (SETTINGS, w - 340, 54, 144, 30),
            (MODE, w - 184, 54, 172, 200),
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
            (OUTPUT, 204, list_top, w - 216, h - list_top - 100),
            (STATUS, 12, h - 88, w - 160, 80),
            (NEXT, w - 140, h - 56, 128, 32),
        ] {
            MoveWindow(control(hwnd, id), x, y, cw, ch, 1);
        }
    }
}
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    match msg {
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
                    (SETTINGS, 0) => {
                        app.setup = !app.setup;
                        layout(hwnd, app.setup);
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
                            app.navigate(previous, false);
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
        child(hwnd, "STATIC", "Каталог:", 0, 201);
        let root = "*".to_string();
        child(hwnd, "EDIT", &root, WS_TABSTOP | 0x80, ROOT);
        child(hwnd, "BUTTON", "Индексировать", WS_TABSTOP, BUILD);
        child(hwnd, "BUTTON", "Все диски", WS_TABSTOP, ALL);
        child(hwnd, "BUTTON", "Обновить", WS_TABSTOP, LOAD);
        child(hwnd, "BUTTON", "←", WS_TABSTOP, BACK);
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
        child(hwnd, "BUTTON", "Открыть", WS_TABSTOP, OPEN);
        child(hwnd, "BUTTON", "Расположение", WS_TABSTOP, LOCATION);
        child(hwnd, "BUTTON", "Параметры", WS_TABSTOP, SETTINGS);
        child(hwnd, "STATIC", "Поиск:", 0, 202);
        child(hwnd, "EDIT", "", WS_TABSTOP | 0x80, QUERY);
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
            WS_TABSTOP | WS_BORDER | LVS_REPORT | LVS_SINGLESEL | LVS_SHOWSELALWAYS,
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
            setup: true,
            sidebar,
            status: String::new(),
            plan: None,
            choice: None,
            saved: super::settings::Preference::load(&index_path().with_extension("preferences")),
            plan_saved: false,
            plan_root: "*".into(),
        };
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
        APP.with(|a| *a.borrow_mut() = Some(app));
        let window = hwnd as usize;
        std::thread::spawn(move || worker(rx, tx, generation, window));
        layout(hwnd, true);
        ShowWindow(hwnd, SW_SHOW);
        SetFocus(control(hwnd, QUERY));
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
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
            if IsDialogMessageW(hwnd, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        APP.with(|a| {
            a.borrow_mut().take();
        });
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
                setup: false,
                sidebar: vec![None],
                status: String::new(),
                plan: None,
                choice: Some(MemoryMode::Fast),
                saved: None,
                plan_saved: false,
                plan_root: "*".into(),
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
