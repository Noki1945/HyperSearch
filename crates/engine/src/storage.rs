use crate::{Entry, Volume};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
};
fn bad() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "corrupt or unsupported index; rebuild required",
    )
}
fn checksum(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf29ce484222325, |h, x| {
        (h ^ *x as u64).wrapping_mul(0x100000001b3)
    })
}
fn number(b: &mut Vec<u8>, n: u64) {
    b.extend(n.to_le_bytes());
}
fn string(b: &mut Vec<u8>, s: &str) {
    number(b, s.len() as u64);
    b.extend(s.as_bytes());
}
struct Reader<'a>(&'a [u8]);
impl Reader<'_> {
    fn number(&mut self) -> io::Result<u64> {
        if self.0.len() < 8 {
            return Err(bad());
        }
        let n = u64::from_le_bytes(self.0[..8].try_into().unwrap());
        self.0 = &self.0[8..];
        Ok(n)
    }
    fn string(&mut self) -> io::Result<String> {
        let n = usize::try_from(self.number()?).map_err(|_| bad())?;
        if n > self.0.len() {
            return Err(bad());
        }
        let s = String::from_utf8(self.0[..n].to_vec()).map_err(|_| bad())?;
        self.0 = &self.0[n..];
        Ok(s)
    }
}
pub fn save(path: &Path, volumes: &[Volume]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut b = b"HYPERS01".to_vec();
    number(&mut b, volumes.len() as u64);
    for v in volumes {
        string(&mut b, &v.root);
        number(&mut b, v.journal);
        number(&mut b, v.next_usn as u64);
        number(&mut b, v.warnings.len() as u64);
        for w in &v.warnings {
            string(&mut b, w);
        }
        number(&mut b, v.entries.len() as u64);
        for e in &v.entries {
            number(&mut b, e.id);
            number(&mut b, e.parent);
            number(&mut b, e.directory as u64);
            string(&mut b, &e.name);
        }
    }
    let hash = checksum(&b);
    number(&mut b, hash);
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    let result = (|| {
        file.write_all(&b)?;
        file.sync_all()?;
        drop(file);
        replace(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
#[cfg(windows)]
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let f: Vec<_> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let t: Vec<_> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe { MoveFileExW(f.as_ptr(), t.as_ptr(), 9) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}
pub fn load(path: &Path) -> io::Result<Vec<Volume>> {
    let mut b = Vec::new();
    File::open(path)?.read_to_end(&mut b)?;
    if b.len() < 24 || &b[..8] != b"HYPERS01" {
        return Err(bad());
    }
    let end = b.len() - 8;
    if checksum(&b[..end]) != u64::from_le_bytes(b[end..].try_into().unwrap()) {
        return Err(bad());
    }
    let mut r = Reader(&b[8..end]);
    let count = r.number()?;
    if count > 26_000 {
        return Err(bad());
    }
    let mut volumes = Vec::new();
    for _ in 0..count {
        let root = r.string()?;
        let journal = r.number()?;
        let next_usn = r.number()? as i64;
        let count = r.number()?;
        if count > r.0.len() as u64 / 8 {
            return Err(bad());
        }
        let mut warnings = Vec::new();
        for _ in 0..count {
            warnings.push(r.string()?);
        }
        let count = r.number()?;
        if count > r.0.len() as u64 / 32 {
            return Err(bad());
        }
        let mut entries = Vec::new();
        for _ in 0..count {
            let id = r.number()?;
            let parent = r.number()?;
            let dir = r.number()?;
            if dir > 1 {
                return Err(bad());
            }
            entries.push(Entry {
                id,
                parent,
                directory: dir == 1,
                name: r.string()?,
            });
        }
        let v = Volume {
            root,
            journal,
            next_usn,
            entries,
            warnings,
        };
        verify(&v)?;
        volumes.push(v);
    }
    if !r.0.is_empty() {
        return Err(bad());
    }
    Ok(volumes)
}
pub fn verify(v: &Volume) -> io::Result<()> {
    let lookup = v.paths();
    if lookup.len() != v.entries.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "duplicate file reference numbers",
        ));
    }
    // Linear-time graph validation avoids quadratic verification of deep directory trees.
    let mut done = std::collections::HashSet::new();
    for e in &v.entries {
        let mut chain = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut id = e.id;
        while !done.contains(&id) {
            if !seen.insert(id) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("parent cycle at FRN {id}"),
                ));
            }
            let e = &v.entries[*lookup.get(&id).ok_or_else(bad)?];
            chain.push(id);
            if e.parent == e.id {
                if !e.directory {
                    return Err(bad());
                }
                break;
            }
            let p = &v.entries[*lookup.get(&e.parent).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("missing parent FRN {} for FRN {}", e.parent, e.id),
                )
            })?];
            if !p.directory {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("parent FRN {} is not a directory", e.parent),
                ));
            }
            id = e.parent;
        }
        done.extend(chain);
    }
    Ok(())
}
