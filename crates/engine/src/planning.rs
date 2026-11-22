//! Low-memory estimates: only filename statistics and posting frequencies are retained.
use crate::search::MemoryMode;
use std::{
    collections::HashMap,
    fs::File,
    io::{self, BufReader, Read},
    path::{Path, PathBuf},
};
#[derive(Default, Clone)]
pub struct Summary {
    pub entries: usize,
    pub searchable: usize,
    pub warnings: usize,
    pub disk_bytes: u64,
    original_alloc: usize,
    lower_bytes: usize,
    lookup_bytes: usize,
    short: HashMap<u32, usize>,
    grams: HashMap<[u8; 3], usize>,
}
fn slots(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        n.saturating_mul(8).div_ceil(7).max(4).next_power_of_two()
    }
}
impl Summary {
    fn observe(&mut self, name: &str, root: bool) {
        self.entries += 1;
        self.original_alloc += name.len().div_ceil(16) * 16 + 16;
        if root {
            return;
        }
        self.searchable += 1;
        let lower = name.to_lowercase();
        self.lower_bytes += lower.len();
        let b = lower.as_bytes();
        let mut short: Vec<_> = b
            .windows(1)
            .map(|x| 0x10000 | x[0] as u32)
            .chain(
                b.windows(2)
                    .map(|x| 0x20000 | ((x[0] as u32) << 8) | x[1] as u32),
            )
            .collect();
        short.sort_unstable();
        short.dedup();
        for k in short {
            *self.short.entry(k).or_default() += 1;
        }
        let mut grams: Vec<_> = b.windows(3).map(|x| [x[0], x[1], x[2]]).collect();
        grams.sort_unstable();
        grams.dedup();
        for k in grams {
            *self.grams.entry(k).or_default() += 1;
        }
    }
    /// Conservative process estimate; not an OS-enforced memory limit.
    pub fn estimate_bytes(&self, mode: MemoryMode) -> usize {
        let base = self.entries * 48
            + self.original_alloc
            + self.lower_bytes
            + self.searchable * 36
            + self.lookup_bytes;
        let bitmap = self.searchable.div_ceil(64) * 8;
        let posting = |counts: &HashMap<_, usize>| {
            counts.values().map(|&n| (n * 4).min(bitmap)).sum::<usize>() + slots(counts.len()) * 49
        };
        let grams = if mode == MemoryMode::Economy {
            0
        } else {
            self.grams
                .values()
                .map(|&n| (n * 4).min(bitmap))
                .sum::<usize>()
                + slots(self.grams.len()) * 49
        };
        let short = if mode == MemoryMode::Fast {
            posting(&self.short)
        } else {
            0
        };
        (base + grams + short) * 115 / 100 + 32 * 1024 * 1024
    }
    pub fn budget_mode(&self, mb: usize) -> io::Result<MemoryMode> {
        for mode in [MemoryMode::Fast, MemoryMode::Balanced, MemoryMode::Economy] {
            if self.estimate_bytes(mode).saturating_mul(120) / 100 <= mb.saturating_mul(1024 * 1024)
            {
                return Ok(mode);
            }
        }
        Err(io::Error::other(format!(
            "Бюджет слишком мал: для этого набора ориентир от {} МБ. Выберите больший бюджет или отдельную папку.",
            self.estimate_bytes(MemoryMode::Economy)
                .div_ceil(1024 * 1024)
                * 120
                / 100
        )))
    }
    pub fn description(&self) -> String {
        let mut s = format!(
            "{} объектов; предупреждений: {}. Оценка RAM с запасом (не точный замер; пик загрузки выше):",
            self.entries, self.warnings
        );
        for (m, label) in [
            (MemoryMode::Economy, "Экономный"),
            (MemoryMode::Balanced, "Сбалансированный"),
            (MemoryMode::Fast, "Быстрый"),
        ] {
            s.push_str(&format!(
                "\r\n{label}: около {} МБ",
                self.estimate_bytes(m).div_ceil(1024 * 1024)
            ));
        }
        if let Some((total, available)) = system_memory() {
            s.push_str(&format!(
                "\r\nRAM компьютера: {} МБ; доступно сейчас: {} МБ.",
                total / 1048576,
                available / 1048576
            ));
        }
        s
    }
}
pub fn system_memory() -> Option<(u64, u64)> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct Status {
            length: u32,
            load: u32,
            total: u64,
            available: u64,
            page_total: u64,
            page_available: u64,
            virtual_total: u64,
            virtual_available: u64,
            extended: u64,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GlobalMemoryStatusEx(status: *mut Status) -> i32;
        }
        let mut status: Status = unsafe { std::mem::zeroed() };
        status.length = std::mem::size_of::<Status>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut status) } != 0 {
            return Some((status.total, status.available));
        }
    }
    None
}
fn number(r: &mut impl Read) -> io::Result<u64> {
    let mut b = [0; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}
fn string(r: &mut impl Read) -> io::Result<String> {
    let n = number(r)?;
    if n > 1024 * 1024 {
        return Err(io::Error::other("invalid index string length"));
    }
    let mut b = vec![0; n as usize];
    r.read_exact(&mut b)?;
    String::from_utf8(b).map_err(io::Error::other)
}
/// This estimates allocation sizes, not integrity; the normal loader verifies the checksum later.
pub fn analyze_index(path: &Path) -> io::Result<Summary> {
    let mut r = BufReader::with_capacity(65536, File::open(path)?);
    let mut magic = [0; 8];
    r.read_exact(&mut magic)?;
    if &magic != b"HYPERS01" {
        return Err(io::Error::other("unsupported index"));
    }
    let mut s = Summary {
        disk_bytes: std::fs::metadata(path)?.len(),
        ..Default::default()
    };
    let volumes = number(&mut r)?;
    if volumes > 26000 {
        return Err(io::Error::other("invalid volume count"));
    }
    for _ in 0..volumes {
        string(&mut r)?;
        number(&mut r)?;
        number(&mut r)?;
        let warnings = number(&mut r)?;
        if warnings > s.disk_bytes / 8 {
            return Err(io::Error::other("invalid warnings"));
        }
        s.warnings += warnings as usize;
        for _ in 0..warnings {
            string(&mut r)?;
        }
        let n = number(&mut r)? as usize;
        if n > s.disk_bytes as usize / 32 {
            return Err(io::Error::other("invalid record count"));
        }
        s.lookup_bytes += slots(n) * 17;
        for _ in 0..n {
            let id = number(&mut r)?;
            let parent = number(&mut r)?;
            number(&mut r)?;
            s.observe(&string(&mut r)?, id == parent);
        }
    }
    number(&mut r)?;
    Ok(s)
}
/// Safe directory traversal for a first launch without an existing index. Does not build or save an index.
pub fn analyze_roots(roots: &[String]) -> io::Result<Summary> {
    let mut s = Summary::default();
    for root in roots {
        let path = PathBuf::from(root).canonicalize()?;
        let before = s.entries;
        s.observe("", true);
        let mut stack = vec![path];
        while let Some(path) = stack.pop() {
            let dir = match std::fs::read_dir(&path) {
                Ok(d) => d,
                Err(_) => {
                    s.warnings += 1;
                    continue;
                }
            };
            for item in dir {
                let item = match item {
                    Ok(i) => i,
                    Err(_) => {
                        s.warnings += 1;
                        continue;
                    }
                };
                let Ok(name) = item.file_name().into_string() else {
                    s.warnings += 1;
                    continue;
                };
                s.observe(&name, false);
                let ty = match item.file_type() {
                    Ok(t) => t,
                    Err(_) => {
                        s.warnings += 1;
                        continue;
                    }
                };
                #[cfg(windows)]
                let reparse = {
                    use std::os::windows::fs::MetadataExt;
                    item.path()
                        .symlink_metadata()
                        .map(|m| m.file_attributes() & 0x400 != 0)
                        .unwrap_or(true)
                };
                #[cfg(not(windows))]
                let reparse = ty.is_symlink();
                if ty.is_dir() && !reparse {
                    stack.push(item.path());
                }
            }
        }
        s.lookup_bytes += slots(s.entries - before) * 17;
    }
    Ok(s)
}
