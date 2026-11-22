//! Persistent indexing scope. Directory anchors remain available for USN parent links.
use crate::{Entry, Volume};
use std::{
    collections::HashMap,
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};
#[derive(Default, Clone)]
pub struct Exclusions {
    paths: Vec<String>,
}
impl Exclusions {
    pub fn new(paths: &[PathBuf]) -> Self {
        let mut normalized = Vec::new();
        for path in paths {
            if let Some(path) = crate::storage_analysis::canonical_prefix(path) {
                normalized.push(crate::storage_analysis::normalized(&path));
            }
            normalized.push(crate::storage_analysis::normalized(path));
        }
        normalized.sort();
        normalized.dedup();
        Self { paths: normalized }
    }
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }
    pub fn contains(&self, path: &str) -> bool {
        if self.is_empty() {
            return false;
        }
        let path = crate::storage_analysis::normalized(Path::new(path));
        self.paths.iter().any(|root| {
            path == *root
                || path
                    .strip_prefix(root)
                    .is_some_and(|rest| rest.starts_with('\\'))
        })
    }
    pub fn bytes(&self) -> usize {
        self.paths.capacity() * std::mem::size_of::<String>()
            + self.paths.iter().map(String::capacity).sum::<usize>()
    }
}
pub fn load(snapshot: &Path) -> io::Result<Vec<PathBuf>> {
    let file = match fs::File::open(snapshot.with_extension("indexing")) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e),
    };
    let mut bytes = Vec::new();
    file.take(1_048_577).read_to_end(&mut bytes)?;
    if bytes.len() > 1_048_576 {
        return Err(io::Error::other("indexing preferences too large"));
    }
    let paths: Vec<String> = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if paths.len() > 1000 || paths.iter().any(|p| !Path::new(p).is_absolute()) {
        return Err(io::Error::other(
            "indexing exclusions must be absolute paths (maximum 1000)",
        ));
    }
    Ok(paths.into_iter().map(PathBuf::from).collect())
}
pub fn save(snapshot: &Path, paths: &[PathBuf]) -> io::Result<()> {
    if paths.len() > 1000 || paths.iter().any(|p| !p.is_absolute()) {
        return Err(io::Error::other("Используйте абсолютные пути исключений"));
    }
    let path = snapshot.with_extension("indexing");
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("indexing.tmp");
    let values: Vec<_> = paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    fs::write(&tmp, serde_json::to_vec(&values)?)?;
    crate::storage::replace(&tmp, &path)
}
/// Removes excluded files. All directory names remain structural anchors, hidden by search policy.
pub fn filter(volumes: &mut [Volume], scope: &Exclusions) -> usize {
    if scope.is_empty() {
        return 0;
    }
    let mut removed = 0;
    for volume in volumes {
        let lookup = volume.paths();
        let keep: Vec<_> = volume
            .entries
            .iter()
            .map(|e| {
                e.directory
                    || volume
                        .path(e.id, &lookup)
                        .is_some_and(|p| !scope.contains(&p))
            })
            .collect();
        let mut i = 0;
        volume.entries.retain(|_| {
            let keep = keep[i];
            i += 1;
            if !keep {
                removed += 1;
            }
            keep
        });
    }
    removed
}
pub(crate) fn new_path(
    volume: &Volume,
    lookup: &HashMap<u64, usize>,
    entry: &Entry,
) -> Option<String> {
    volume
        .path(entry.parent, lookup)
        .map(|parent| format!("{}\\{}", parent.trim_end_matches('\\'), entry.name))
}
pub(crate) fn crosses(
    volume: &Volume,
    lookup: &HashMap<u64, usize>,
    entry: &Entry,
    reason: u32,
    scope: &Exclusions,
) -> bool {
    if scope.is_empty()
        || !entry.directory
        || entry.id == entry.parent
        || reason & 0x200 != 0
        || !lookup.contains_key(&entry.id)
    {
        return false;
    }
    let old = volume.path(entry.id, lookup);
    let new = new_path(volume, lookup, entry);
    match (old, new) {
        (Some(old), Some(new)) => scope.contains(&old) != scope.contains(&new),
        (Some(old), None) => scope.contains(&old),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{MemoryMode, Mode, Query, SearchIndex};
    fn entry(id: u64, parent: u64, name: &str, directory: bool) -> Entry {
        Entry {
            id,
            parent,
            name: name.into(),
            directory,
        }
    }
    #[test]
    fn excluded_files_and_anchors_remain_hidden_through_updates() {
        for mode in [MemoryMode::Economy, MemoryMode::Balanced, MemoryMode::Fast] {
            let mut volumes = vec![Volume {
                root: "C:\\".into(),
                journal: 1,
                next_usn: 0,
                warnings: vec![],
                entries: vec![
                    entry(1, 1, "", true),
                    entry(2, 1, "Private", true),
                    entry(3, 2, "Secret.exe", false),
                    entry(4, 1, "Public", true),
                    entry(5, 4, "Visible.exe", false),
                ],
            }];
            let paths = [PathBuf::from("C:\\Private")];
            let scope = Exclusions::new(&paths);
            assert!(!scope.contains("C:\\Private-other\\x"));
            assert!(scope.contains("c:\\PRIVATE\\x"));
            assert_eq!(filter(&mut volumes, &scope), 1);
            assert!(volumes[0].entries.iter().any(|e| e.id == 2));
            let mut index = SearchIndex::with_mode(volumes, mode);
            index.set_exclusions(&paths);
            let find = |index: &SearchIndex, text| {
                index
                    .search(
                        &Query {
                            text,
                            mode: Mode::Substring,
                            extension: None,
                            path: None,
                        },
                        None,
                        100,
                    )
                    .ids
            };
            assert!(find(&index, "secret").is_empty());
            assert!(find(&index, "private").is_empty());
            assert_eq!(find(&index, "visible").len(), 1);
            index
                .apply_checked(0, vec![(entry(6, 2, "New.exe", false), 0x100)])
                .unwrap();
            assert!(find(&index, "new.exe").is_empty());
            index
                .apply_checked(0, vec![(entry(6, 4, "New.exe", false), 0x2000)])
                .unwrap();
            assert_eq!(find(&index, "new.exe").len(), 1);
            index
                .apply_checked(0, vec![(entry(5, 2, "Visible.exe", false), 0x2000)])
                .unwrap();
            assert!(find(&index, "visible").is_empty());
            assert!(
                index
                    .apply_checked(0, vec![(entry(2, 4, "Moved", true), 0x2000)])
                    .is_err()
            );
        }
    }
    #[test]
    fn preferences_round_trip_and_invalid_config_fails_closed() {
        let dir = std::env::temp_dir().join(format!("hypersearch-scope-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let snapshot = dir.join("index.hsi");
        assert!(load(&snapshot).unwrap().is_empty());
        let paths = vec![dir.join("private")];
        save(&snapshot, &paths).unwrap();
        assert_eq!(load(&snapshot).unwrap(), paths);
        assert!(save(&snapshot, &[PathBuf::from("relative")]).is_err());
        fs::write(snapshot.with_extension("indexing"), b"broken").unwrap();
        assert!(load(&snapshot).is_err());
        fs::remove_dir_all(&dir).unwrap();
    }
}
