//! Read-only, documented volume IOCTLs. All output is parsed as bytes, never cast to aligned structs.
use crate::{Entry, Volume};
use std::{ffi::c_void, io, mem::size_of, os::windows::ffi::OsStrExt};
type Handle = *mut c_void;
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateFileW(
        name: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CloseHandle(h: Handle) -> i32;
    fn OpenFileById(
        volume: Handle,
        id: *const FileId,
        access: u32,
        share: u32,
        security: *const c_void,
        flags: u32,
    ) -> Handle;
    fn GetFinalPathNameByHandleW(h: Handle, path: *mut u16, len: u32, flags: u32) -> u32;
    fn GetFileInformationByHandle(h: Handle, info: *mut FileInfo) -> i32;
    fn DeviceIoControl(
        h: Handle,
        code: u32,
        input: *const c_void,
        input_len: u32,
        output: *mut u8,
        output_len: u32,
        returned: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
}
pub struct VolumeHandle(Handle);
#[repr(C)]
struct FileId {
    size: u32,
    kind: u32,
    id: [u64; 2],
}
#[repr(C)]
#[derive(Default)]
struct FileInfo {
    attributes: u32,
    created: [u32; 2],
    accessed: [u32; 2],
    written: [u32; 2],
    serial: u32,
    size_high: u32,
    size_low: u32,
    links: u32,
    id_high: u32,
    id_low: u32,
}
impl Drop for VolumeHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
#[repr(C)]
struct EnumData {
    start: u64,
    low: i64,
    high: i64,
}
#[repr(C)]
struct ReadData {
    start: i64,
    reason: u32,
    close: u32,
    timeout: u64,
    wait: u64,
    journal: u64,
}
fn invalid(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, s)
}
fn u16at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap())
}
fn u32at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}
fn u64at(b: &[u8], p: usize) -> u64 {
    u64::from_le_bytes(b[p..p + 8].try_into().unwrap())
}
pub fn parse_records(b: &[u8]) -> io::Result<Vec<(Entry, u32)>> {
    let mut pos = 0;
    let mut entries = Vec::new();
    while pos < b.len() {
        if b.len() - pos < 60 {
            return Err(invalid("truncated USN record"));
        }
        let r = &b[pos..];
        let len = u32at(r, 0) as usize;
        if len < 60 || len > r.len() {
            return Err(invalid("invalid USN record length"));
        }
        if u16at(r, 4) != 2 {
            return Err(invalid("unsupported USN record version; require V2 NTFS"));
        }
        let n = u16at(r, 56) as usize;
        let off = u16at(r, 58) as usize;
        if !n.is_multiple_of(2) || off < 60 || off + n > len {
            return Err(invalid("invalid USN filename"));
        }
        let wide: Vec<_> = r[off..off + n]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|x| u16::from_le_bytes([x[0], x[1]]))
            .collect();
        let name = String::from_utf16(&wide).map_err(|_| invalid("unpaired UTF-16 surrogate"))?;
        entries.push((
            Entry {
                id: u64at(r, 8),
                parent: u64at(r, 16),
                name,
                directory: u32at(r, 52) & 0x10 != 0,
            },
            u32at(r, 40),
        ));
        pos += len;
    }
    Ok(entries)
}
impl VolumeHandle {
    /// Enumeration can omit unchanged NTFS metadata directories. Resolve only missing
    /// parents, never issue an expensive path lookup for each enumerated file.
    fn parent_entry(&self, id: u64) -> io::Result<Entry> {
        let descriptor = FileId {
            size: size_of::<FileId>() as u32,
            kind: 0,
            id: [id, 0],
        };
        let file = unsafe { OpenFileById(self.0, &descriptor, 0, 7, std::ptr::null(), 0x02000000) };
        if file as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        let file = VolumeHandle(file);
        let mut path = vec![0u16; 32768];
        let n =
            unsafe { GetFinalPathNameByHandleW(file.0, path.as_mut_ptr(), path.len() as u32, 0) };
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n as usize >= path.len() {
            return Err(invalid("parent path too long"));
        }
        let path = std::path::PathBuf::from(
            String::from_utf16(&path[..n as usize])
                .map_err(|_| invalid("invalid parent Unicode"))?,
        );
        let Some(name) = path.file_name() else {
            return Ok(Entry {
                id,
                parent: id,
                name: String::new(),
                directory: true,
            });
        };
        let parent = path
            .parent()
            .ok_or_else(|| invalid("parent path missing"))?;
        let wide: Vec<_> = parent.as_os_str().encode_wide().chain(Some(0)).collect();
        let h = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0,
                7,
                std::ptr::null(),
                3,
                0x02000000,
                std::ptr::null_mut(),
            )
        };
        if h as isize == -1 {
            return Err(io::Error::last_os_error());
        }
        let h = VolumeHandle(h);
        let mut info = FileInfo::default();
        if unsafe { GetFileInformationByHandle(h.0, &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Entry {
            id,
            parent: ((info.id_high as u64) << 32) | info.id_low as u64,
            name: name
                .to_str()
                .ok_or_else(|| invalid("invalid parent name"))?
                .into(),
            directory: true,
        })
    }
    pub fn open(root: &str) -> io::Result<Self> {
        if root.len() != 3 || root.as_bytes()[1] != b':' {
            return Err(invalid("expected drive root X:\\"));
        }
        let wide: Vec<_> = std::ffi::OsStr::new(&format!("\\\\.\\{}", &root[..2]))
            .encode_wide()
            .chain(Some(0))
            .collect();
        let h = unsafe {
            CreateFileW(
                wide.as_ptr(),
                0x80000000,
                7,
                std::ptr::null(),
                3,
                0,
                std::ptr::null_mut(),
            )
        };
        if h as isize == -1 {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(h))
        }
    }
    fn ioctl<T>(&self, code: u32, input: &T) -> io::Result<Vec<u8>> {
        let mut out = vec![0; 1024 * 1024];
        let mut returned = 0;
        let ok = unsafe {
            DeviceIoControl(
                self.0,
                code,
                input as *const T as *const c_void,
                size_of::<T>() as u32,
                out.as_mut_ptr(),
                out.len() as u32,
                &mut returned,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        out.truncate(returned as usize);
        Ok(out)
    }
    pub fn journal(&self) -> io::Result<(u64, i64, i64)> {
        let b = self.ioctl(0x000900f4, &())?;
        if b.len() < 56 {
            return Err(invalid("truncated journal metadata"));
        }
        Ok((
            u64at(&b, 0),
            (u64at(&b, 8) as i64).max(u64at(&b, 24) as i64),
            u64at(&b, 16) as i64,
        ))
    }
    pub fn changes(&self, journal: u64, start: i64) -> io::Result<(i64, Vec<(Entry, u32)>)> {
        let b = self.ioctl(
            0x000900bb,
            &ReadData {
                start,
                reason: u32::MAX,
                close: 0,
                timeout: 0,
                wait: 0,
                journal,
            },
        )?;
        if b.len() < 8 {
            return Err(invalid("truncated journal buffer"));
        }
        Ok((u64at(&b, 0) as i64, parse_records(&b[8..])?))
    }
}
pub fn enumerate(root: &str) -> io::Result<Volume> {
    let h = VolumeHandle::open(root)?;
    let (journal, _, next_usn) = h.journal()?;
    let mut data = EnumData {
        start: 0,
        low: 0,
        high: i64::MAX,
    };
    let mut entries = Vec::new();
    loop {
        let b = match h.ioctl(0x000900b3, &data) {
            Ok(b) => b,
            Err(e) if e.raw_os_error() == Some(38) => break,
            Err(e) => return Err(e),
        };
        if b.len() < 8 {
            return Err(invalid("truncated enumeration buffer"));
        }
        let next = u64at(&b, 0);
        entries.extend(parse_records(&b[8..])?.into_iter().map(|(e, _)| e));
        if next <= data.start {
            return Err(invalid("enumeration did not advance"));
        }
        data.start = next;
    }
    // NTFS root has MFT record number 5; preserve its sequence-number bits.
    if !entries.iter().any(|e| e.id == e.parent) {
        let root_id = entries
            .iter()
            .map(|e| e.parent)
            .find(|id| id & 0x0000ffffffffffff == 5)
            .ok_or_else(|| invalid("NTFS root FRN missing"))?;
        entries.push(Entry {
            id: root_id,
            parent: root_id,
            name: String::new(),
            directory: true,
        });
    }
    let mut v = Volume {
        root: root.into(),
        journal,
        next_usn,
        entries,
        warnings: vec![
            "USN enumeration exposes one name per FRN; additional hard-link names require --walk"
                .into(),
        ],
    };
    crate::monitor::catch_up(&mut v)?;
    let mut present: std::collections::HashSet<_> = v.entries.iter().map(|e| e.id).collect();
    let mut pending: Vec<_> = v
        .entries
        .iter()
        .map(|e| e.parent)
        .filter(|id| !present.contains(id))
        .collect();
    let mut resolved = 0;
    while let Some(id) = pending.pop() {
        if present.contains(&id) {
            continue;
        }
        let e = h.parent_entry(id)?;
        present.insert(id);
        if !present.contains(&e.parent) {
            pending.push(e.parent);
        }
        v.entries.push(e);
        resolved += 1;
    }
    if resolved > 0 {
        v.warnings.push(format!(
            "Resolved {resolved} parents omitted by USN enumeration"
        ));
    }
    crate::storage::verify(&v)?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_records() {
        assert!(parse_records(&[0; 59]).is_err());
        assert!(parse_records(&[0; 60]).is_err());
    }
    #[test]
    fn unicode_record() {
        let wide: Vec<u16> = "Отчёт.xlsx".encode_utf16().collect();
        let len = 60 + wide.len() * 2;
        let mut b = vec![0; len];
        b[..4].copy_from_slice(&(len as u32).to_le_bytes());
        b[4..6].copy_from_slice(&2u16.to_le_bytes());
        b[8..16].copy_from_slice(&42u64.to_le_bytes());
        b[16..24].copy_from_slice(&5u64.to_le_bytes());
        b[40..44].copy_from_slice(&0x2000u32.to_le_bytes());
        b[56..58].copy_from_slice(&((wide.len() * 2) as u16).to_le_bytes());
        b[58..60].copy_from_slice(&60u16.to_le_bytes());
        for (i, c) in wide.iter().enumerate() {
            b[60 + i * 2..62 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        let records = parse_records(&b).unwrap();
        assert_eq!(records[0].0.name, "Отчёт.xlsx");
        assert_eq!(records[0].0.id, 42);
        assert_eq!(records[0].1, 0x2000);
    }
}
