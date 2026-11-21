use hypersearch_engine::{
    Entry,
    benchmark::synthetic,
    monitor,
    search::{Mode, Query, SearchIndex},
    storage,
};
#[test]
fn indexed_matches_linear_and_pages() {
    let index = SearchIndex::new(vec![synthetic(10_000)]);
    for memory in [
        hypersearch_engine::search::MemoryMode::Economy,
        hypersearch_engine::search::MemoryMode::Balanced,
        hypersearch_engine::search::MemoryMode::Fast,
    ] {
        let index = SearchIndex::with_mode(index.volumes.clone(), memory);
        for text in [
            "report",
            "report 2026",
            "ОТПУСК",
            ".xlsx",
            "0",
            "00",
            "000",
            "absent",
            "zz",
            "fi",
            "ф",
            "",
        ] {
            for mode in [Mode::Exact, Mode::Prefix, Mode::Substring] {
                let q = Query {
                    text,
                    mode,
                    extension: None,
                    path: None,
                };
                let t = text.to_lowercase();
                let expected: Vec<_> = (0..index.len() as u32)
                    .filter(|&i| {
                        let n = index.name(i).to_lowercase();
                        match mode {
                            Mode::Exact => n == t,
                            Mode::Prefix => n.starts_with(&t),
                            Mode::Substring => t.split_whitespace().all(|s| n.contains(s)),
                        }
                    })
                    .collect();
                let mut actual = Vec::new();
                let mut after = None;
                loop {
                    let p = index.search(&q, after, 37);
                    actual.extend(p.ids);
                    match p.next {
                        Some(n) => after = Some(n),
                        None => break,
                    }
                }
                assert_eq!(actual, expected, "{mode:?} {text}");
            }
        }
    }
}
#[test]
fn filters_and_zero_limit() {
    let index = SearchIndex::new(vec![synthetic(100)]);
    let q = Query {
        text: "report",
        mode: Mode::Substring,
        extension: Some("XLSX"),
        path: Some("C:\\bench"),
    };
    assert_eq!(index.search(&q, None, 100).ids.len(), 25);
    assert!(index.search(&q, None, 0).ids.is_empty());
    let q = Query {
        path: Some("C:\\ben"),
        ..q
    };
    assert!(index.search(&q, None, 100).ids.is_empty());
}
#[test]
fn storage_roundtrip_corruption_and_atomic_replace() {
    let dir = std::env::temp_dir().join(format!("hypersearch-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("index.hsi");
    let v = synthetic(100);
    storage::save(&p, std::slice::from_ref(&v)).unwrap();
    storage::save(&p, &[v]).unwrap();
    let loaded = storage::load(&p).unwrap();
    assert_eq!(loaded[0].entries.len(), 101);
    let mut bytes = std::fs::read(&p).unwrap();
    bytes[20] ^= 1;
    std::fs::write(&p, bytes).unwrap();
    assert!(storage::load(&p).is_err());
    std::fs::remove_file(&p).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
#[test]
fn journal_rename_move_delete() {
    let mut v = synthetic(2);
    monitor::apply(
        &mut v,
        vec![(
            Entry {
                id: 1,
                parent: 0,
                name: "Новый.xlsx".into(),
                directory: false,
            },
            0x2000,
        )],
    );
    assert_eq!(v.entries[1].name, "Новый.xlsx");
    monitor::apply(
        &mut v,
        vec![(
            Entry {
                id: 1,
                parent: 0,
                name: "Новый.xlsx".into(),
                directory: false,
            },
            0x200,
        )],
    );
    assert_eq!(v.entries.len(), 2);
    storage::verify(&v).unwrap();
}
#[test]
fn graph_cycles_and_missing_parents() {
    let mut v = synthetic(2);
    v.entries[1].parent = 999;
    assert!(storage::verify(&v).is_err());
    v.entries[1].parent = 2;
    v.entries[2].parent = 1;
    v.entries[1].directory = true;
    v.entries[2].directory = true;
    assert!(storage::verify(&v).is_err());
}
#[test]
fn fallback_unicode_and_junction_safe() {
    let dir = std::env::temp_dir().join(format!("hypersearch-walk-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("Отчёт_2026.txt"), b"test").unwrap();
    let v = hypersearch_engine::filesystem::walk(&dir).unwrap();
    storage::verify(&v).unwrap();
    let index = SearchIndex::new(vec![v]);
    let p = index.search(
        &Query {
            text: "ОТЧЁТ",
            mode: Mode::Substring,
            extension: None,
            path: None,
        },
        None,
        100,
    );
    assert_eq!(p.ids.len(), 1);
    assert!(index.path(p.ids[0]).unwrap().ends_with("Отчёт_2026.txt"));
    std::fs::remove_file(dir.join("Отчёт_2026.txt")).unwrap();
    std::fs::remove_dir(dir).unwrap();
}
#[test]
fn extended_paths_and_moved_parent() {
    let mut v = synthetic(1);
    v.root = "\\\\?\\C:\\bench\\".into();
    v.entries.push(Entry {
        id: 2,
        parent: 0,
        name: "Старый".into(),
        directory: true,
    });
    v.entries[1].parent = 2;
    monitor::apply(
        &mut v,
        vec![(
            Entry {
                id: 2,
                parent: 0,
                name: "Новый".into(),
                directory: true,
            },
            0x2000,
        )],
    );
    let index = SearchIndex::new(vec![v]);
    let q = Query {
        text: "report",
        mode: Mode::Substring,
        extension: None,
        path: Some("C:/bench/НОВЫЙ"),
    };
    assert_eq!(index.search(&q, None, 10).ids.len(), 1);
}
#[test]
fn planning_budget_and_streamed_counts() {
    use hypersearch_engine::{planning, search::MemoryMode};
    let path = std::env::temp_dir().join(format!("hypersearch-plan-{}.hsi", std::process::id()));
    let v = synthetic(1000);
    storage::save(&path, &[v]).unwrap();
    let plan = planning::analyze_index(&path).unwrap();
    assert_eq!(plan.entries, 1001);
    assert_eq!(plan.searchable, 1000);
    assert!(plan.estimate_bytes(MemoryMode::Economy) < plan.estimate_bytes(MemoryMode::Balanced));
    assert!(plan.estimate_bytes(MemoryMode::Balanced) < plan.estimate_bytes(MemoryMode::Fast));
    assert!(plan.budget_mode(1).is_err());
    assert_eq!(plan.budget_mode(1024).unwrap(), MemoryMode::Fast);
    std::fs::remove_file(path).unwrap();
}
#[test]
fn exact_duplicate_names_and_cancel() {
    use hypersearch_engine::search::MemoryMode;
    let mut v = synthetic(300);
    v.entries[1].name = "EXCEL.EXE".into();
    v.entries[250].name = "excel.exe".into();
    for mode in [MemoryMode::Economy, MemoryMode::Balanced, MemoryMode::Fast] {
        let index = SearchIndex::with_mode(vec![v.clone()], mode);
        let q = Query {
            text: "Excel.Exe",
            mode: Mode::Exact,
            extension: None,
            path: None,
        };
        let p = index.search(&q, None, 1);
        assert_eq!(p.ids, vec![0]);
        assert_eq!(index.search(&q, p.next, 1).ids, vec![249]);
        assert!(
            index
                .search_cancellable(&q, None, 100, || true)
                .ids
                .is_empty()
        );
    }
}
#[test]
fn folder_browsing_direct_children_and_pages() {
    let mut v = synthetic(3);
    v.entries.push(Entry {
        id: 4,
        parent: 0,
        name: "Папка".into(),
        directory: true,
    });
    v.entries[1].parent = 4;
    let index = SearchIndex::new(vec![v]);
    let root = index.children("C:\\bench", None, 2).unwrap();
    assert_eq!(root.ids, vec![1, 2]);
    assert_eq!(
        index.children("C:\\bench", root.next, 2).unwrap().ids,
        vec![3]
    );
    assert_eq!(
        index.children("C:\\bench\\ПАПКА", None, 100).unwrap().ids,
        vec![0]
    );
    assert!(index.children("C:\\missing", None, 100).is_none());
}
