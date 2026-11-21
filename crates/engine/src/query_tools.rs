//! Optional presentation filters and ranking. Core ID cursors remain unchanged.
use crate::search::{Mode, Page, Query, SearchIndex};
use std::collections::HashMap;

#[derive(Default, Debug, PartialEq)]
pub struct Filters {
    pub text: String,
    pub extensions: Vec<String>,
    pub category: Option<String>,
    pub path: Option<String>,
    pub ranked: bool,
}
impl Filters {
    /// Quoted paths and names are accepted; unrecognized tokens remain literal text.
    pub fn parse(input: &str) -> Self {
        let mut tokens = Vec::new();
        let mut token = String::new();
        let mut quoted = false;
        for c in input.chars() {
            if c == '"' {
                quoted = !quoted;
            } else if c.is_whitespace() && !quoted {
                if !token.is_empty() {
                    tokens.push(std::mem::take(&mut token));
                }
            } else {
                token.push(c);
            }
        }
        if !token.is_empty() {
            tokens.push(token);
        }
        let mut filters = Self::default();
        let mut words = Vec::new();
        for token in tokens {
            if let Some(v) = token.strip_prefix("ext:") {
                filters.extensions.extend(
                    v.split(',')
                        .filter(|s| !s.is_empty())
                        .map(|s| s.trim_start_matches('.').to_lowercase()),
                );
            } else if let Some(v) = token.strip_prefix("type:") {
                filters.category = Some(v.to_lowercase());
            } else if let Some(v) = token.strip_prefix("in:") {
                filters.path = Some(v.into());
            } else if token == "rank:on" {
                filters.ranked = true;
            } else {
                words.push(token);
            }
        }
        filters.text = words.join(" ");
        filters
    }
    fn accepts(&self, index: &SearchIndex, id: u32) -> bool {
        let name = index.name(id);
        let ext = std::path::Path::new(name)
            .extension()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !self.extensions.is_empty() && !self.extensions.contains(&ext) {
            return false;
        }
        match self.category.as_deref() {
            None | Some("all") => true,
            Some("folders") => index.is_directory(id),
            Some("files") => !index.is_directory(id),
            Some("documents") => matches!(
                ext.as_str(),
                "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "odt"
            ),
            Some("images") => matches!(
                ext.as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "svg"
            ),
            Some("video") => matches!(ext.as_str(), "mp4" | "mkv" | "mov" | "avi" | "webm"),
            Some("audio") => matches!(ext.as_str(), "mp3" | "wav" | "flac" | "m4a" | "ogg"),
            Some("archives") => matches!(ext.as_str(), "zip" | "7z" | "rar" | "tar" | "gz" | "iso"),
            Some("apps") => matches!(ext.as_str(), "exe" | "msi" | "com" | "bat" | "cmd"),
            _ => false,
        }
    }
}
pub fn search(
    index: &SearchIndex,
    filters: &Filters,
    mode: Mode,
    scope: Option<&str>,
    after: Option<u32>,
    limit: usize,
    cancelled: impl Fn() -> bool,
) -> Page {
    let mut result = Page {
        ids: Vec::new(),
        next: None,
        examined: 0,
    };
    let mut cursor = after;
    loop {
        let page = index.search_cancellable(
            &Query {
                text: &filters.text,
                mode,
                extension: None,
                path: filters.path.as_deref().or(scope),
            },
            cursor,
            256,
            &cancelled,
        );
        result.examined += page.examined;
        for id in page.ids {
            if cancelled() {
                return result;
            }
            if filters.accepts(index, id) {
                if result.ids.len() == limit {
                    result.next = result.ids.last().copied();
                    return result;
                }
                result.ids.push(id);
            }
        }
        cursor = page.next;
        if cursor.is_none() || cancelled() {
            return result;
        }
    }
}
/// Reorders a bounded display page, never changes the engine cursor or candidate set.
pub fn rank(index: &SearchIndex, ids: &mut [u32], query: &str, opened: &HashMap<String, u32>) {
    let query = query.to_lowercase();
    ids.sort_by_cached_key(|&id| {
        let name = index.name(id).to_lowercase();
        let relevance = if name == query {
            0
        } else if name.starts_with(&query) {
            1
        } else if name
            .split(|c: char| !c.is_alphanumeric())
            .any(|w| w.starts_with(&query))
        {
            2
        } else {
            3
        };
        let frequency = index
            .path(id)
            .and_then(|p| opened.get(&p).copied())
            .unwrap_or(0);
        (relevance, std::cmp::Reverse(frequency), name, id)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoted_filter_and_pagination() {
        let f = Filters::parse("отчёт ext:.XLSX,pdf in:\"C:\\Work Files\" rank:on");
        assert_eq!(f.text, "отчёт");
        assert_eq!(f.path.as_deref(), Some("C:\\Work Files"));
        assert_eq!(f.extensions, ["xlsx", "pdf"]);
        let index = SearchIndex::new(vec![crate::benchmark::synthetic(900)]);
        let f = Filters::parse("ext:xlsx");
        let first = search(&index, &f, Mode::Substring, None, None, 3, || false);
        assert_eq!(first.ids.len(), 3);
        let second = search(&index, &f, Mode::Substring, None, first.next, 3, || false);
        assert!(first.ids.iter().all(|id| !second.ids.contains(id)));
        let mut ranked = first.ids.clone();
        rank(&index, &mut ranked, "", &HashMap::new());
        ranked.sort();
        assert_eq!(ranked, first.ids);
    }
}
