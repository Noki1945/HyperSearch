use hypersearch_engine::{
    benchmark::synthetic,
    search::{MemoryMode, Mode, Query, SearchIndex},
};
fn query() -> Query<'static> {
    Query {
        text: "report 2026",
        mode: Mode::Substring,
        extension: None,
        path: None,
    }
}
#[test]
fn roundtrip_profiles_stale_source_corruption_and_checkpoint() {
    let dir = std::env::temp_dir().join(format!("hypersearch-cache-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for mode in [MemoryMode::Economy, MemoryMode::Balanced, MemoryMode::Fast] {
        let path = dir.join(format!("{}.hsc", mode.key()));
        let v = synthetic(4097);
        let (fresh, report) = SearchIndex::with_disk_cache(vec![v.clone()], mode, &path);
        assert!(!report.hit && report.saved);
        let mut same = v.clone();
        same.journal = 42;
        same.next_usn = 800;
        let (cached, report) = SearchIndex::with_disk_cache(vec![same], mode, &path);
        assert!(report.hit);
        let mut after = None;
        loop {
            let a = fresh.search(&query(), after, 37);
            let b = cached.search(&query(), after, 37);
            assert_eq!(a.ids, b.ids);
            assert_eq!(a.next, b.next);
            for &id in &a.ids {
                assert_eq!(fresh.path(id), cached.path(id));
            }
            match a.next {
                Some(n) => after = Some(n),
                None => break,
            }
        }
        let mut changed = v.clone();
        changed.entries[1].name = "different-file.txt".into();
        let (_, report) = SearchIndex::with_disk_cache(vec![changed.clone()], mode, &path);
        assert!(!report.hit && report.saved);
        let (_, report) = SearchIndex::with_disk_cache(vec![changed.clone()], mode, &path);
        assert!(report.hit);
        let mut bytes = std::fs::read(&path).unwrap();
        let end = bytes.len() - 1;
        bytes[end] ^= 1;
        std::fs::write(&path, bytes).unwrap();
        let (_, report) = SearchIndex::with_disk_cache(vec![changed], mode, &path);
        assert!(!report.hit && report.saved);
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_dir(dir).unwrap();
}
#[test]
fn incompatible_mode_empty_index_and_unwritable_cache() {
    let dir = std::env::temp_dir().join(format!("hypersearch-cache-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tables.hsc");
    let (_, r) = SearchIndex::with_disk_cache(vec![], MemoryMode::Economy, &path);
    assert!(r.saved && !r.hit);
    let (i, r) = SearchIndex::with_disk_cache(vec![], MemoryMode::Economy, &path);
    assert!(r.hit && i.is_empty());
    let (_, r) = SearchIndex::with_disk_cache(vec![], MemoryMode::Fast, &path);
    assert!(!r.hit && r.saved);
    let blocked = path.join("child.hsc");
    let (i, r) = SearchIndex::with_disk_cache(vec![synthetic(50)], MemoryMode::Fast, &blocked);
    assert!(!r.saved);
    assert!(!i.search(&query(), None, 10).ids.is_empty());
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
#[test]
fn application_files_do_not_invalidate_cache() {
    use hypersearch_engine::{Entry, state_files};
    let dir = std::env::temp_dir().join(format!("hypersearch-state-filter-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let snapshot = dir.join("index.hsi");
    let mut v = synthetic(1);
    v.root = format!("{}\\", dir.display());
    v.entries.push(Entry {
        id: 2,
        parent: 0,
        name: "index.fast.hsc".into(),
        directory: false,
    });
    v.entries.push(Entry {
        id: 3,
        parent: 0,
        name: "index.hsi".into(),
        directory: false,
    });
    v.entries.push(Entry {
        id: 4,
        parent: 0,
        name: "keep.txt".into(),
        directory: false,
    });
    let mut volumes = vec![v];
    state_files::exclude(&mut volumes, &snapshot);
    assert_eq!(volumes[0].entries.len(), 3);
    assert!(volumes[0].entries.iter().any(|e| e.name == "keep.txt"));
    std::fs::remove_dir(dir).unwrap();
}
