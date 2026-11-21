use crate::Volume;
use std::{collections::HashMap, ops::Index};
#[path = "search_cache.rs"]
mod cache;
pub use cache::{CacheReport, cache_path};
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Exact,
    Prefix,
    Substring,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryMode {
    Economy,
    Balanced,
    Fast,
}
impl MemoryMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "economy" => Some(Self::Economy),
            "balanced" => Some(Self::Balanced),
            "fast" => Some(Self::Fast),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Self::Economy => "economy",
            Self::Balanced => "balanced",
            Self::Fast => "fast",
        }
    }
}
pub struct Query<'a> {
    pub text: &'a str,
    pub mode: Mode,
    pub extension: Option<&'a str>,
    pub path: Option<&'a str>,
}
pub struct Page {
    pub ids: Vec<u32>,
    pub next: Option<u32>,
    pub examined: usize,
}
/// One contiguous allocation for normalized names, without duplicated exact-map keys.
pub(crate) struct Names {
    blob: String,
    spans: Vec<(usize, usize)>,
}
impl Names {
    fn push(&mut self, s: &str) {
        let start = self.blob.len();
        self.blob.push_str(s);
        self.spans.push((start, self.blob.len()));
    }
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.spans.iter().map(|&(a, b)| &self.blob[a..b])
    }
    fn len(&self) -> usize {
        self.spans.len()
    }
}
impl Index<usize> for Names {
    type Output = str;
    fn index(&self, i: usize) -> &str {
        let (a, b) = self.spans[i];
        &self.blob[a..b]
    }
}
enum Posting {
    Sparse(Vec<u32>),
    Dense { bits: Vec<u64>, len: usize },
}
impl Posting {
    fn add(&mut self, id: u32, total: usize) {
        match self {
            Self::Sparse(ids) => {
                match ids.binary_search(&id) {
                    Ok(_) => return,
                    Err(at) => ids.insert(at, id),
                }
                if ids.len() > total.div_ceil(32).max(16) {
                    let mut bits = vec![0; total.div_ceil(64)];
                    for &i in ids.iter() {
                        bits[i as usize / 64] |= 1 << (i % 64);
                    }
                    *self = Self::Dense {
                        bits,
                        len: ids.len(),
                    };
                }
            }
            Self::Dense { bits, len } => {
                bits.resize(bits.len().max(id as usize / 64 + 1), 0);
                if bits[id as usize / 64] & (1 << (id % 64)) != 0 {
                    return;
                }
                bits[id as usize / 64] |= 1 << (id % 64);
                *len += 1;
            }
        }
    }
    fn remove(&mut self, id: u32) {
        match self {
            Self::Sparse(ids) => {
                if let Ok(at) = ids.binary_search(&id) {
                    ids.remove(at);
                }
            }
            Self::Dense { bits, len } => {
                if let Some(word) = bits.get_mut(id as usize / 64)
                    && *word & (1 << (id % 64)) != 0
                {
                    *word &= !(1 << (id % 64));
                    *len -= 1;
                }
            }
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Sparse(v) => v.len(),
            Self::Dense { len, .. } => *len,
        }
    }
    fn compact(&mut self) {
        if let Self::Sparse(v) = self {
            v.shrink_to_fit();
        }
    }
    fn bytes(&self) -> usize {
        match self {
            Self::Sparse(v) => v.capacity() * 4,
            Self::Dense { bits, .. } => bits.capacity() * 8,
        }
    }
    fn from(&self, start: usize) -> Box<dyn Iterator<Item = u32> + '_> {
        match self {
            Self::Sparse(ids) => Box::new(
                ids[ids.partition_point(|&id| (id as usize) < start)..]
                    .iter()
                    .copied(),
            ),
            Self::Dense { bits, .. } => Box::new(
                bits.iter()
                    .enumerate()
                    .skip(start / 64)
                    .flat_map(move |(i, &word)| {
                        let mut word = if i == start / 64 {
                            word & (u64::MAX << (start % 64))
                        } else {
                            word
                        };
                        std::iter::from_fn(move || {
                            if word == 0 {
                                None
                            } else {
                                let bit = word.trailing_zeros();
                                word &= word - 1;
                                Some((i * 64 + bit as usize) as u32)
                            }
                        })
                    }),
            ),
        }
    }
}
pub struct SearchIndex {
    pub volumes: Vec<Volume>,
    pub(crate) names: Names,
    locations: Vec<(u32, u32)>,
    sorted: Vec<u32>,
    short: HashMap<u32, Posting>,
    grams: HashMap<[u8; 3], Posting>,
    lookups: Vec<HashMap<u64, usize>>,
    ids_by_entry: Vec<Vec<Option<u32>>>,
    pub memory_mode: MemoryMode,
}
fn grams(s: &str) -> Vec<[u8; 3]> {
    let mut g: Vec<_> = s
        .as_bytes()
        .windows(3)
        .map(|w| [w[0], w[1], w[2]])
        .collect();
    g.sort_unstable();
    g.dedup();
    g
}
fn short_key(b: &[u8]) -> u32 {
    if b.len() == 1 {
        0x10000 | b[0] as u32
    } else {
        0x20000 | ((b[0] as u32) << 8) | b[1] as u32
    }
}
fn normalized_path(path: &str) -> String {
    let path = path.replace('/', "\\").to_lowercase();
    let path = path
        .strip_prefix("\\\\?\\unc\\")
        .map(|s| format!("\\\\{s}"))
        .unwrap_or_else(|| path.strip_prefix("\\\\?\\").unwrap_or(&path).to_string());
    path.trim_end_matches('\\').to_string()
}
impl SearchIndex {
    pub fn new(volumes: Vec<Volume>) -> Self {
        Self::with_mode(volumes, MemoryMode::Fast)
    }
    pub fn with_mode(mut volumes: Vec<Volume>, mode: MemoryMode) -> Self {
        let total: usize = volumes
            .iter()
            .map(|v| v.entries.iter().filter(|e| e.id != e.parent).count())
            .sum();
        assert!(total <= u32::MAX as usize, "too many index records");
        for v in &mut volumes {
            v.entries.shrink_to_fit();
        }
        let mut s = Self {
            ids_by_entry: volumes
                .iter()
                .map(|v| vec![None; v.entries.len()])
                .collect(),
            lookups: volumes.iter().map(Volume::paths).collect(),
            volumes,
            names: Names {
                blob: String::new(),
                spans: Vec::with_capacity(total),
            },
            locations: Vec::with_capacity(total),
            sorted: Vec::new(),
            short: HashMap::new(),
            grams: HashMap::new(),
            memory_mode: mode,
        };
        for (vi, v) in s.volumes.iter().enumerate() {
            for (ei, e) in v.entries.iter().enumerate() {
                if e.id == e.parent {
                    continue;
                }
                let id = s.names.len() as u32;
                let name = e.name.to_lowercase();
                if mode == MemoryMode::Fast {
                    let mut keys: Vec<_> = name
                        .as_bytes()
                        .windows(1)
                        .chain(name.as_bytes().windows(2))
                        .map(short_key)
                        .collect();
                    keys.sort_unstable();
                    keys.dedup();
                    for k in keys {
                        s.short
                            .entry(k)
                            .or_insert_with(|| Posting::Sparse(Vec::new()))
                            .add(id, total);
                    }
                }
                if mode != MemoryMode::Economy {
                    for g in grams(&name) {
                        s.grams
                            .entry(g)
                            .or_insert_with(|| Posting::Sparse(Vec::new()))
                            .add(id, total);
                    }
                }
                s.names.push(&name);
                s.locations.push((vi as u32, ei as u32));
                s.ids_by_entry[vi][ei] = Some(id);
            }
        }
        s.names.blob.shrink_to_fit();
        s.sorted = (0..total as u32).collect();
        s.sorted.sort_unstable_by(|&a, &b| {
            s.names[a as usize]
                .cmp(&s.names[b as usize])
                .then(a.cmp(&b))
        });
        for p in s.short.values_mut().chain(s.grams.values_mut()) {
            p.compact();
        }
        s
    }
    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.len() == 0
    }
    /// Owned allocation model, excluding allocator metadata, UI and temporary peaks.
    pub fn allocation_bytes(&self) -> usize {
        let records: usize = self
            .volumes
            .iter()
            .map(|v| {
                v.entries.capacity() * std::mem::size_of::<crate::Entry>()
                    + v.entries.iter().map(|e| e.name.capacity()).sum::<usize>()
            })
            .sum();
        let lookups: usize = self.lookups.iter().map(|m| m.capacity() * 20).sum();
        records
            + lookups
            + self.names.blob.capacity()
            + self.names.spans.capacity() * 16
            + self.locations.capacity() * 8
            + self.sorted.capacity() * 4
            + self
                .short
                .values()
                .chain(self.grams.values())
                .map(Posting::bytes)
                .sum::<usize>()
            + (self.short.capacity() + self.grams.capacity()) * 48
    }
    pub fn path(&self, id: u32) -> Option<String> {
        let &(v, e) = self.locations.get(id as usize)?;
        if self.names[id as usize].is_empty() {
            return None;
        }
        let v = v as usize;
        self.volumes[v].path(self.volumes[v].entries[e as usize].id, &self.lookups[v])
    }
    pub fn name(&self, id: u32) -> &str {
        let (v, e) = self.locations[id as usize];
        &self.volumes[v as usize].entries[e as usize].name
    }
    pub fn is_directory(&self, id: u32) -> bool {
        let (v, e) = self.locations[id as usize];
        self.volumes[v as usize].entries[e as usize].directory
    }
    /// Direct indexed children. Returns None when the directory is not in this snapshot.
    pub fn children(&self, path: &str, after: Option<u32>, limit: usize) -> Option<Page> {
        let wanted = normalized_path(path);
        let leaf = wanted.rsplit('\\').next().unwrap_or("");
        let mut parent = None;
        for (vi, v) in self.volumes.iter().enumerate() {
            for e in &v.entries {
                if !e.directory || (e.id != e.parent && e.name.to_lowercase() != leaf) {
                    continue;
                }
                if v.path(e.id, &self.lookups[vi])
                    .is_some_and(|p| normalized_path(&p) == wanted)
                {
                    parent = Some((vi, e.id));
                    break;
                }
            }
            if parent.is_some() {
                break;
            }
        }
        let (vi, parent) = parent?;
        let mut page = Page {
            ids: Vec::new(),
            next: None,
            examined: 0,
        };
        if limit == 0 {
            return Some(page);
        }
        for id in after.map_or(0, |i| i as usize + 1)..self.locations.len() {
            let (v, e) = self.locations[id];
            if v as usize != vi {
                continue;
            }
            page.examined += 1;
            let e = &self.volumes[vi].entries[e as usize];
            if e.parent == parent && e.id != parent && !e.name.is_empty() {
                if page.ids.len() == limit {
                    page.next = page.ids.last().copied();
                    break;
                }
                page.ids.push(id as u32);
            }
        }
        Some(page)
    }
    pub fn search(&self, q: &Query<'_>, after: Option<u32>, limit: usize) -> Page {
        self.search_cancellable(q, after, limit, || false)
    }
    /// A cancelled page is partial and must be discarded by the caller.
    pub fn search_cancellable(
        &self,
        q: &Query<'_>,
        after: Option<u32>,
        limit: usize,
        cancelled: impl Fn() -> bool,
    ) -> Page {
        let empty = || Page {
            ids: vec![],
            next: None,
            examined: 0,
        };
        if limit == 0 {
            return empty();
        }
        let text = q.text.to_lowercase();
        let tokens: Vec<_> = text.split_whitespace().collect();
        let ext = q
            .extension
            .map(|x| format!(".{}", x.trim_start_matches('.').to_lowercase()));
        let path = q.path.map(normalized_path);
        let start = after.map_or(0, |x| x as usize + 1);
        let mut best: Option<&Posting> = None;
        if q.mode != Mode::Exact {
            let words = if q.mode == Mode::Prefix {
                vec![text.as_str()]
            } else {
                tokens.clone()
            };
            for t in words {
                if t.is_empty() {
                    continue;
                }
                if t.len() < 3 && self.memory_mode == MemoryMode::Fast {
                    let Some(p) = self.short.get(&short_key(t.as_bytes())) else {
                        return empty();
                    };
                    if best.is_none_or(|b| p.len() < b.len()) {
                        best = Some(p);
                    }
                }
                if t.len() >= 3 && self.memory_mode != MemoryMode::Economy {
                    for g in grams(t) {
                        let Some(p) = self.grams.get(&g) else {
                            return empty();
                        };
                        if best.is_none_or(|b| p.len() < b.len()) {
                            best = Some(p);
                        }
                    }
                }
            }
        }
        let candidates: Box<dyn Iterator<Item = u32>> = if q.mode == Mode::Exact {
            let a = self
                .sorted
                .partition_point(|&id| self.names[id as usize] < *text);
            let b = self
                .sorted
                .partition_point(|&id| self.names[id as usize] <= *text);
            let ids = &self.sorted[a..b];
            Box::new(
                ids[ids.partition_point(|&id| (id as usize) < start)..]
                    .iter()
                    .copied(),
            )
        } else if let Some(p) = best {
            p.from(start)
        } else {
            Box::new((start..self.len()).map(|i| i as u32))
        };
        let mut result = empty();
        for id in candidates {
            if result.examined & 255 == 0 && cancelled() {
                break;
            }
            result.examined += 1;
            let n = &self.names[id as usize];
            if n.is_empty() {
                continue;
            }
            let matches = match q.mode {
                Mode::Exact => n == text,
                Mode::Prefix => n.starts_with(&text),
                Mode::Substring => tokens.iter().all(|t| n.contains(t)),
            };
            if !matches {
                continue;
            }
            let (v, e) = self.locations[id as usize];
            if ext.as_ref().is_some_and(|x| {
                !n.ends_with(x) || self.volumes[v as usize].entries[e as usize].directory
            }) {
                continue;
            }
            if path.as_ref().is_some_and(|p| {
                !self.path(id).is_some_and(|s| {
                    let s = normalized_path(&s);
                    s == *p || s.starts_with(&format!("{p}\\"))
                })
            }) {
                continue;
            }
            if result.ids.len() == limit {
                result.next = result.ids.last().copied();
                break;
            }
            result.ids.push(id);
        }
        result
    }
    /// Mutates only changed names/postings. Stable IDs survive rename and deletion.
    pub fn apply_changes(&mut self, volume: usize, changes: Vec<(crate::Entry, u32)>) {
        for (entry, reason) in changes {
            if reason & 0x1000 != 0 && reason & 0x2000 == 0 {
                continue;
            }
            let existing = self.lookups[volume].get(&entry.id).copied();
            let deleted = reason & 0x200 != 0;
            let ei = match existing {
                Some(ei) => ei,
                None if deleted => continue,
                None => {
                    let ei = self.volumes[volume].entries.len();
                    self.lookups[volume].insert(entry.id, ei);
                    self.volumes[volume].entries.push(entry.clone());
                    self.ids_by_entry[volume].push(None);
                    ei
                }
            };
            if entry.id == entry.parent {
                self.volumes[volume].entries[ei] = entry;
                continue;
            }
            let id = if let Some(id) = self.ids_by_entry[volume][ei] {
                id
            } else {
                let id = self.names.len() as u32;
                self.names.push("");
                self.locations.push((volume as u32, ei as u32));
                self.ids_by_entry[volume][ei] = Some(id);
                id
            };
            let old = self.names[id as usize].to_string();
            let new = if deleted {
                String::new()
            } else {
                entry.name.to_lowercase()
            };
            if old != new {
                if let Ok(at) = self.sorted.binary_search_by(|&other| {
                    self.names[other as usize]
                        .cmp(old.as_str())
                        .then(other.cmp(&id))
                }) {
                    self.sorted.remove(at);
                }
                if self.memory_mode == MemoryMode::Fast {
                    for key in old
                        .as_bytes()
                        .windows(1)
                        .chain(old.as_bytes().windows(2))
                        .map(short_key)
                    {
                        if let Some(p) = self.short.get_mut(&key) {
                            p.remove(id);
                        }
                    }
                    for key in new
                        .as_bytes()
                        .windows(1)
                        .chain(new.as_bytes().windows(2))
                        .map(short_key)
                    {
                        self.short
                            .entry(key)
                            .or_insert_with(|| Posting::Sparse(Vec::new()))
                            .add(id, self.names.len());
                    }
                }
                if self.memory_mode != MemoryMode::Economy {
                    for key in grams(&old) {
                        if let Some(p) = self.grams.get_mut(&key) {
                            p.remove(id);
                        }
                    }
                    for key in grams(&new) {
                        self.grams
                            .entry(key)
                            .or_insert_with(|| Posting::Sparse(Vec::new()))
                            .add(id, self.names.len());
                    }
                }
                let start = self.names.blob.len();
                self.names.blob.push_str(&new);
                self.names.spans[id as usize] = (start, start + new.len());
                if !new.is_empty() {
                    let at = self.sorted.partition_point(|&other| {
                        self.names[other as usize]
                            .cmp(new.as_str())
                            .then(other.cmp(&id))
                            .is_lt()
                    });
                    self.sorted.insert(at, id);
                }
            }
            self.volumes[volume].entries[ei] = entry;
            if deleted {
                self.volumes[volume].entries[ei].name.clear();
                self.lookups[volume].remove(&self.volumes[volume].entries[ei].id);
            }
        }
    }
}
