use hypersearch_engine::{
    Entry, Volume,
    search::{MemoryMode, Mode, Query, SearchIndex},
};
fn entry(id: u64, parent: u64, name: &str, directory: bool) -> Entry {
    Entry {
        id,
        parent,
        name: name.into(),
        directory,
    }
}

#[test]
fn dense_postings_match_fresh_index_after_many_updates() {
    for mode in [MemoryMode::Economy, MemoryMode::Balanced, MemoryMode::Fast] {
        let mut volume = hypersearch_engine::benchmark::synthetic(1200);
        let mut index = SearchIndex::with_mode(vec![volume.clone()], mode);
        for step in 1..100 {
            let id = 1 + ((step * 97) % 1200) as u64;
            let reason = if step % 3 == 0 { 0x200 } else { 0x2000 };
            let change = entry(id, 0, &format!("Фото_Excel_{step}.exe"), false);
            index.apply_changes(0, vec![(change.clone(), reason)]);
            hypersearch_engine::monitor::apply(&mut volume, vec![(change, reason)]);
        }
        let reference = SearchIndex::with_mode(vec![volume], mode);
        for mode in [Mode::Exact, Mode::Prefix, Mode::Substring] {
            for text in ["Фото", "ex", "f", "Фото_Excel_99.exe", "report", "zz", ""] {
                let query = Query {
                    text,
                    mode,
                    extension: None,
                    path: None,
                };
                let paths = |index: &SearchIndex| {
                    let mut paths: Vec<_> = index
                        .search(&query, None, 10000)
                        .ids
                        .into_iter()
                        .filter_map(|id| index.path(id))
                        .collect();
                    paths.sort();
                    paths
                };
                assert_eq!(paths(&index), paths(&reference), "{mode:?} {text}");
            }
        }
    }
}
#[test]
fn incremental_stable_ids_all_memory_modes() {
    for mode in [MemoryMode::Economy, MemoryMode::Balanced, MemoryMode::Fast] {
        let volume = Volume {
            root: "C:\\".into(),
            journal: 1,
            next_usn: 0,
            warnings: vec![],
            entries: vec![
                entry(1, 1, "", true),
                entry(2, 1, "Docs", true),
                entry(3, 2, "Отчёт.xlsx", false),
            ],
        };
        let mut index = SearchIndex::with_mode(vec![volume], mode);
        let find = |index: &SearchIndex, text: &str, mode| {
            index
                .search(
                    &Query {
                        text,
                        mode,
                        extension: None,
                        path: None,
                    },
                    None,
                    100,
                )
                .ids
        };
        let original = find(&index, "Отчёт.xlsx", Mode::Exact)[0];
        index.apply_changes(
            0,
            vec![
                (entry(3, 2, "Excel.exe", false), 0x2000),
                (entry(4, 2, "excel-two.exe", false), 0x100),
            ],
        );
        assert!(find(&index, "Отчёт", Mode::Substring).is_empty());
        assert_eq!(find(&index, "Excel.exe", Mode::Exact), [original]);
        assert_eq!(find(&index, "ex", Mode::Prefix).len(), 2);
        index.apply_changes(0, vec![(entry(2, 1, "Moved", true), 0x2000)]);
        assert_eq!(
            index.path(original).as_deref(),
            Some("C:\\Moved\\Excel.exe")
        );
        index.apply_changes(0, vec![(entry(3, 2, "Excel.exe", false), 0x200)]);
        assert!(find(&index, "Excel.exe", Mode::Exact).is_empty());
        assert!(index.path(original).is_none());
        assert_eq!(find(&index, "ex", Mode::Substring).len(), 1);
        index.apply_changes(0, vec![(entry(5, 1, "EXCEL.EXE", false), 0x100)]);
        assert_eq!(find(&index, "excel.exe", Mode::Exact).len(), 1);
    }
}
