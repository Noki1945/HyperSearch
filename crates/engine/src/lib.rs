pub mod benchmark;
pub mod filesystem;
pub mod indexer;
pub mod metrics;
pub mod monitor;
#[cfg(windows)]
pub mod ntfs;
pub mod planning;
pub mod search;
pub mod state_files;
pub mod storage;

use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: u64,
    pub parent: u64,
    pub name: String,
    pub directory: bool,
}

#[derive(Clone, Debug)]
pub struct Volume {
    pub root: String,
    pub journal: u64,
    pub next_usn: i64,
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

impl Volume {
    pub fn paths(&self) -> HashMap<u64, usize> {
        self.entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.id, i))
            .collect()
    }
    pub fn path(&self, id: u64, lookup: &HashMap<u64, usize>) -> Option<String> {
        let mut parts = Vec::new();
        let mut current = id;
        for _ in 0..=self.entries.len() {
            let e = self.entries.get(*lookup.get(&current)?)?;
            if e.parent == e.id {
                parts.reverse();
                return Some(format!("{}{}", self.root, parts.join("\\")));
            }
            parts.push(e.name.as_str());
            current = e.parent;
        }
        None
    }
}
