//! Bounded metadata-only cache. Every reuse first validates live metadata, never a TTL guess.
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Record {
    pub text: String,
    pub size: Option<u64>,
    pub modified: Option<u64>,
}
#[derive(Serialize, Deserialize)]
struct Cached {
    fingerprint: (u64, u64, u64, u32),
    record: Record,
}
#[derive(Default, Serialize, Deserialize)]
pub struct Cache {
    values: HashMap<String, Cached>,
}
impl Cache {
    pub fn load(path: &Path) -> Self {
        std::fs::File::open(path)
            .ok()
            .and_then(|f| {
                use std::io::Read;
                let mut b = Vec::new();
                f.take(4_000_001).read_to_end(&mut b).ok()?;
                if b.len() > 4_000_000 {
                    return None;
                }
                serde_json::from_slice::<Self>(&b)
                    .ok()
                    .filter(|c| c.values.len() <= 2048)
            })
            .unwrap_or_default()
    }
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec(self)?)
    }
    pub fn read(&mut self, path: &str) -> Record {
        use std::os::windows::fs::MetadataExt;
        let m = match std::fs::symlink_metadata(path) {
            Ok(m) => m,
            Err(e) => {
                self.values.remove(path);
                return Record {
                    text: format!("{path}\r\nМетаданные недоступны: {e}"),
                    ..Record::default()
                };
            }
        };
        let fingerprint = (
            m.file_size(),
            m.last_write_time(),
            m.creation_time(),
            m.file_attributes(),
        );
        if let Some(c) = self.values.get(path)
            && c.fingerprint == fingerprint
        {
            return c.record.clone();
        }
        let size = if m.is_dir() || m.file_attributes() & 0x400 != 0 {
            None
        } else {
            Some(m.len())
        };
        let modified = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let record = Record {
            text: format!(
                "{}\r\n{}\r\nИзменён: {}\r\nСоздан: {}\r\nАтрибуты: 0x{:X}\r\nФизическое место: не измерено",
                path,
                size.map(|n| format!("Логический размер: {n} байт"))
                    .unwrap_or_else(|| "Размер вложений не подсчитан".into()),
                date(m.modified().ok()),
                date(m.created().ok()),
                m.file_attributes()
            ),
            size,
            modified,
        };
        if self.values.len() >= 2048 && !self.values.contains_key(path) {
            self.values.clear();
        }
        self.values.insert(
            path.into(),
            Cached {
                fingerprint,
                record: record.clone(),
            },
        );
        record
    }
}
pub fn date(time: Option<SystemTime>) -> String {
    time.map(|t| super::app::date(Ok(t))).unwrap_or_default()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistent_cache_validates_modified_and_deleted_files() {
        let dir = std::env::temp_dir().join(format!("hs-meta-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("file.txt");
        let key = p.to_string_lossy();
        std::fs::write(&p, b"one").unwrap();
        let mut c = Cache::default();
        assert_eq!(c.read(&key).size, Some(3));
        let cache = dir.join("cache.json");
        c.save(&cache).unwrap();
        let mut c = Cache::load(&cache);
        std::fs::write(&p, b"different").unwrap();
        assert_eq!(c.read(&key).size, Some(9));
        std::fs::remove_file(&p).unwrap();
        assert!(c.read(&key).size.is_none());
        assert!(c.read(&key).text.contains("недоступны"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
