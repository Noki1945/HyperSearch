//! Exclude atomically replaced application state to avoid self-invalidating caches.
use crate::Volume;
use std::path::Path;
fn normalized(s: &str) -> String {
    s.replace('/', "\\")
        .trim_start_matches("\\\\?\\")
        .trim_end_matches('\\')
        .to_lowercase()
}
pub fn exclude(volumes: &mut [Volume], snapshot: &Path) {
    let absolute = if snapshot.is_absolute() {
        snapshot.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(p) => p.join(snapshot),
            Err(_) => return,
        }
    };
    let Some(parent) = absolute.parent() else {
        return;
    };
    let target = normalized(&parent.to_string_lossy());
    let leaf = parent
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase());
    let Some(stem) = absolute
        .file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_lowercase)
    else {
        return;
    };
    let filename = absolute
        .file_name()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    for v in volumes {
        let lookup = v.paths();
        let parents: std::collections::HashSet<_> = v
            .entries
            .iter()
            .filter(|e| {
                e.directory
                    && (e.id == e.parent
                        || leaf.as_ref().is_some_and(|l| e.name.to_lowercase() == *l))
            })
            .filter(|e| {
                v.path(e.id, &lookup)
                    .is_some_and(|p| normalized(&p) == target)
            })
            .map(|e| e.id)
            .collect();
        v.entries.retain(|e| {
            if !parents.contains(&e.parent) || e.directory {
                return true;
            }
            let name = e.name.to_lowercase();
            !(name == filename
                || name == format!("{stem}.preferences")
                || name == format!("{stem}.appearance")
                || name == format!("{stem}.pins")
                || name == format!("{stem}.hotkey")
                || name == format!("{stem}.opens")
                || name == "windirstat.path"
                || name == "windirstat.download"
                || (name.starts_with(&format!("{stem}."))
                    && (name.ends_with(".hsc") || name.ends_with(".tmp"))))
        });
    }
}
