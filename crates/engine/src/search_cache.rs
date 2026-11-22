//! Portable derived-table cache. Raw HashMap memory is never persisted.
use super::{MemoryMode, Names, Posting, SearchIndex};
use crate::Volume;
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    io::{self, BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
};
const MAGIC: &[u8; 8] = b"HYPTBL01";
const SEED: u64 = 0xcbf29ce484222325;
fn hash(mut h: u64, b: &[u8]) -> u64 {
    for &x in b {
        h = (h ^ x as u64).wrapping_mul(0x100000001b3);
    }
    h
}
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid, stale or incompatible search table cache",
    )
}
fn mode_tag(m: MemoryMode) -> u64 {
    match m {
        MemoryMode::Economy => 0,
        MemoryMode::Balanced => 1,
        MemoryMode::Fast => 2,
    }
}
fn fingerprint(volumes: &[Volume]) -> u64 {
    let mut h = hash(SEED, &(volumes.len() as u64).to_le_bytes());
    for v in volumes {
        h = hash(h, &(v.root.len() as u64).to_le_bytes());
        h = hash(h, v.root.as_bytes());
        h = hash(h, &(v.entries.len() as u64).to_le_bytes());
        for e in &v.entries {
            h = hash(h, &e.id.to_le_bytes());
            h = hash(h, &e.parent.to_le_bytes());
            h = hash(h, &[e.directory as u8]);
            h = hash(h, &(e.name.len() as u64).to_le_bytes());
            h = hash(h, e.name.as_bytes());
        }
    }
    h
}
pub fn cache_path(snapshot: &Path, mode: MemoryMode) -> PathBuf {
    snapshot.with_extension(format!("{}.hsc", mode.key()))
}
pub struct CacheReport {
    pub hit: bool,
    pub saved: bool,
    pub detail: String,
}
struct Writer {
    file: BufWriter<File>,
    hash: u64,
}
impl Writer {
    fn bytes(&mut self, b: &[u8]) -> io::Result<()> {
        self.hash = hash(self.hash, b);
        self.file.write_all(b)
    }
    fn number(&mut self, n: u64) -> io::Result<()> {
        self.bytes(&n.to_le_bytes())
    }
    fn u32s(&mut self, ids: &[u32]) -> io::Result<()> {
        self.number(ids.len() as u64)?;
        let mut b = Vec::with_capacity(65536);
        for chunk in ids.chunks(16384) {
            b.clear();
            for n in chunk {
                b.extend(n.to_le_bytes());
            }
            self.bytes(&b)?;
        }
        Ok(())
    }
    fn u64s(&mut self, words: &[u64]) -> io::Result<()> {
        self.number(words.len() as u64)?;
        let mut b = Vec::with_capacity(65536);
        for chunk in words.chunks(8192) {
            b.clear();
            for n in chunk {
                b.extend(n.to_le_bytes());
            }
            self.bytes(&b)?;
        }
        Ok(())
    }
    fn posting(&mut self, p: &Posting) -> io::Result<()> {
        match p {
            Posting::Sparse(ids) => {
                self.number(0)?;
                self.u32s(ids)
            }
            Posting::Dense { bits, len } => {
                self.number(1)?;
                self.number(*len as u64)?;
                self.u64s(bits)
            }
        }
    }
}
struct Reader {
    file: BufReader<File>,
    hash: u64,
    left: u64,
}
impl Reader {
    fn bytes(&mut self, b: &mut [u8]) -> io::Result<()> {
        if b.len() as u64 > self.left {
            return Err(invalid());
        }
        self.file.read_exact(b)?;
        self.left -= b.len() as u64;
        self.hash = hash(self.hash, b);
        Ok(())
    }
    fn number(&mut self) -> io::Result<u64> {
        let mut b = [0; 8];
        self.bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
    fn count(&mut self, max: usize, width: usize) -> io::Result<usize> {
        let n = usize::try_from(self.number()?).map_err(|_| invalid())?;
        if n > max || n as u64 > self.left / width as u64 {
            return Err(invalid());
        }
        Ok(n)
    }
    fn u32s(&mut self, max: usize) -> io::Result<Vec<u32>> {
        let n = self.count(max, 4)?;
        let mut v = Vec::with_capacity(n);
        let mut b = [0; 65536];
        let mut remaining = n;
        while remaining > 0 {
            let count = remaining.min(b.len() / 4);
            self.bytes(&mut b[..count * 4])?;
            v.extend(
                b[..count * 4]
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|x| u32::from_le_bytes(*x)),
            );
            remaining -= count;
        }
        Ok(v)
    }
    fn u64s(&mut self, max: usize) -> io::Result<Vec<u64>> {
        let n = self.count(max, 8)?;
        let mut v = Vec::with_capacity(n);
        let mut b = [0; 65536];
        let mut remaining = n;
        while remaining > 0 {
            let count = remaining.min(b.len() / 8);
            self.bytes(&mut b[..count * 8])?;
            v.extend(
                b[..count * 8]
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .map(|x| u64::from_le_bytes(*x)),
            );
            remaining -= count;
        }
        Ok(v)
    }
    fn posting(&mut self, total: usize) -> io::Result<Posting> {
        match self.number()? {
            0 => {
                let ids = self.u32s(total)?;
                if ids.iter().any(|&id| id as usize >= total)
                    || ids.windows(2).any(|w| w[0] >= w[1])
                {
                    return Err(invalid());
                }
                Ok(Posting::Sparse(ids))
            }
            1 => {
                let len = usize::try_from(self.number()?).map_err(|_| invalid())?;
                let bits = self.u64s(total.div_ceil(64))?;
                if bits.len() != total.div_ceil(64)
                    || len != bits.iter().map(|n| n.count_ones() as usize).sum::<usize>()
                    || len > total
                {
                    return Err(invalid());
                }
                if !total.is_multiple_of(64) && bits.last().is_some_and(|b| b >> (total % 64) != 0)
                {
                    return Err(invalid());
                }
                Ok(Posting::Dense { bits, len })
            }
            _ => Err(invalid()),
        }
    }
}
impl SearchIndex {
    /// Cache failure never prevents searching; it falls back to a fresh build.
    pub fn with_disk_cache(
        mut volumes: Vec<Volume>,
        mode: MemoryMode,
        path: &Path,
    ) -> (Self, CacheReport) {
        for volume in &mut volumes {
            volume.entries.shrink_to_fit();
        }
        let signature = fingerprint(&volumes);
        let loaded = Self::read_cache(&volumes, mode, path, signature);
        match loaded {
            Ok((names, locations, sorted, short, grams)) => {
                let lookups = volumes.iter().map(Volume::paths).collect();
                let mut ids_by_entry: Vec<Vec<Option<u32>>> = volumes
                    .iter()
                    .map(|v| vec![None; v.entries.len()])
                    .collect();
                for (id, &(v, e)) in locations.iter().enumerate() {
                    ids_by_entry[v as usize][e as usize] = Some(id as u32);
                }
                (
                    Self {
                        exclusions: Default::default(),
                        volumes,
                        names,
                        locations,
                        sorted,
                        short,
                        grams,
                        lookups,
                        ids_by_entry,
                        memory_mode: mode,
                    },
                    CacheReport {
                        hit: true,
                        saved: true,
                        detail: "готовые таблицы загружены с диска".into(),
                    },
                )
            }
            Err(error) => {
                let built = Self::with_mode(volumes, mode);
                let saved = built.write_cache(path, signature);
                let report = CacheReport {
                    hit: false,
                    saved: saved.is_ok(),
                    detail: match saved {
                        Ok(()) => format!("таблицы построены и сохранены ({error})"),
                        Err(e) => format!("таблицы построены; сохранить кэш не удалось: {e}"),
                    },
                };
                (built, report)
            }
        }
    }
    fn write_cache(&self, path: &Path, signature: u64) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let result = (|| {
            let file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            let mut w = Writer {
                file: BufWriter::with_capacity(262144, file),
                hash: SEED,
            };
            w.bytes(MAGIC)?;
            w.number(mode_tag(self.memory_mode))?;
            w.number(signature)?;
            w.number(self.len() as u64)?;
            w.number(self.names.blob.len() as u64)?;
            w.bytes(self.names.blob.as_bytes())?;
            w.number(self.names.spans.len() as u64)?;
            for &(a, b) in &self.names.spans {
                w.number(a as u64)?;
                w.number(b as u64)?;
            }
            w.number(self.locations.len() as u64)?;
            for &(v, e) in &self.locations {
                w.number(((v as u64) << 32) | e as u64)?;
            }
            w.u32s(&self.sorted)?;
            w.number(self.short.len() as u64)?;
            for (&key, p) in &self.short {
                w.number(key as u64)?;
                w.posting(p)?;
            }
            w.number(self.grams.len() as u64)?;
            for (&key, p) in &self.grams {
                w.number(u32::from_le_bytes([key[0], key[1], key[2], 0]) as u64)?;
                w.posting(p)?;
            }
            w.file.write_all(&w.hash.to_le_bytes())?;
            w.file.flush()?;
            w.file.get_ref().sync_all()?;
            drop(w);
            crate::storage::replace(&tmp, path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    }
}
type Tables = (
    Names,
    Vec<(u32, u32)>,
    Vec<u32>,
    HashMap<u32, Posting>,
    HashMap<[u8; 3], Posting>,
);
impl SearchIndex {
    fn read_cache(
        volumes: &[Volume],
        mode: MemoryMode,
        path: &Path,
        signature: u64,
    ) -> io::Result<Tables> {
        let file = File::open(path)?;
        let size = file.metadata()?.len();
        if size < 48 {
            return Err(invalid());
        }
        let mut r = Reader {
            file: BufReader::with_capacity(262144, file),
            hash: SEED,
            left: size - 8,
        };
        let mut magic = [0; 8];
        r.bytes(&mut magic)?;
        let total: usize = volumes
            .iter()
            .map(|v| v.entries.iter().filter(|e| e.id != e.parent).count())
            .sum();
        if &magic != MAGIC
            || r.number()? != mode_tag(mode)
            || r.number()? != signature
            || r.number()? != total as u64
        {
            return Err(invalid());
        }
        let bytes = r.count(usize::MAX, 1)?;
        let mut blob = vec![0; bytes];
        r.bytes(&mut blob)?;
        let blob = String::from_utf8(blob).map_err(|_| invalid())?;
        let n = r.count(total, 16)?;
        if n != total {
            return Err(invalid());
        }
        let mut spans = Vec::with_capacity(total);
        let mut previous = 0;
        for _ in 0..n {
            let a = usize::try_from(r.number()?).map_err(|_| invalid())?;
            let b = usize::try_from(r.number()?).map_err(|_| invalid())?;
            if a != previous
                || b < a
                || b > blob.len()
                || !blob.is_char_boundary(a)
                || !blob.is_char_boundary(b)
            {
                return Err(invalid());
            }
            spans.push((a, b));
            previous = b;
        }
        if previous != blob.len() {
            return Err(invalid());
        }
        let names = Names { blob, spans };
        if r.count(total, 8)? != total {
            return Err(invalid());
        }
        let mut locations = Vec::with_capacity(total);
        for (vi, v) in volumes.iter().enumerate() {
            for (ei, e) in v.entries.iter().enumerate() {
                if e.id == e.parent {
                    continue;
                }
                let location = r.number()?;
                if location != ((vi as u64) << 32) | ei as u64 {
                    return Err(invalid());
                }
                locations.push((vi as u32, ei as u32));
            }
        }
        let sorted = r.u32s(total)?;
        if sorted.len() != total {
            return Err(invalid());
        }
        let mut seen = vec![false; total];
        for &id in &sorted {
            if id as usize >= total || std::mem::replace(&mut seen[id as usize], true) {
                return Err(invalid());
            }
        }
        if sorted.windows(2).any(|w| {
            names[w[0] as usize] > names[w[1] as usize]
                || (names[w[0] as usize] == names[w[1] as usize] && w[0] >= w[1])
        }) {
            return Err(invalid());
        }
        drop(seen);
        let count = r.count(65792, 16)?;
        let mut short = HashMap::with_capacity(count);
        for _ in 0..count {
            let key = u32::try_from(r.number()?).map_err(|_| invalid())?;
            if !(0x10000..=0x100ff).contains(&key) && !(0x20000..=0x2ffff).contains(&key) {
                return Err(invalid());
            }
            let p = r.posting(total)?;
            if short.insert(key, p).is_some() {
                return Err(invalid());
            }
        }
        let count = r.count(1 << 24, 16)?;
        let mut grams = HashMap::with_capacity(count);
        for _ in 0..count {
            let key = u32::try_from(r.number()?).map_err(|_| invalid())?;
            if key > 0xffffff {
                return Err(invalid());
            }
            let b = key.to_le_bytes();
            let p = r.posting(total)?;
            if grams.insert([b[0], b[1], b[2]], p).is_some() {
                return Err(invalid());
            }
        }
        if (mode != MemoryMode::Fast && !short.is_empty())
            || (mode == MemoryMode::Economy && !grams.is_empty())
            || r.left != 0
        {
            return Err(invalid());
        }
        let mut tail = [0; 8];
        r.file.read_exact(&mut tail)?;
        if u64::from_le_bytes(tail) != r.hash {
            return Err(invalid());
        }
        Ok((names, locations, sorted, short, grams))
    }
}
