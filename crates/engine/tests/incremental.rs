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
