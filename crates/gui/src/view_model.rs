use hypersearch_engine::search::SearchIndex;
#[derive(Clone, Debug)]
pub struct Row {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub directory: bool,
}
pub fn rows(index: &SearchIndex, ids: &[u32]) -> Vec<Row> {
    ids.iter()
        .filter_map(|&id| {
            let path = index.path(id)?;
            let directory = index.is_directory(id);
            let name = index.name(id).to_string();
            let kind = if directory {
                "Папка".into()
            } else {
                std::path::Path::new(&name)
                    .extension()
                    .map(|e| format!("Файл {}", e.to_string_lossy().to_uppercase()))
                    .unwrap_or_else(|| "Файл".into())
            };
            Some(Row {
                name,
                path,
                kind,
                directory,
            })
        })
        .collect()
}
pub fn parent(path: &str) -> Option<String> {
    std::path::Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().into_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn result_rows() {
        let index = SearchIndex::new(vec![hypersearch_engine::benchmark::synthetic(4)]);
        let r = rows(&index, &[0, 1, 2, 3]);
        assert_eq!(r.len(), 4);
        assert_eq!(r[0].kind, "Файл XLSX");
        assert!(!r[0].directory);
        assert!(r[0].path.ends_with(&r[0].name));
    }
}
