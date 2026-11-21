use crate::{Entry, Volume};
use std::{io, path::Path};

pub fn drives() -> Vec<String> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|c| format!("{}:\\", c as char))
            .filter(|p| Path::new(p).exists())
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec!["/".into()]
    }
}

/// Directory fallback preserves separate hard-link names; never follows reparse points.
pub fn walk(root: &Path) -> io::Result<Volume> {
    let root = root.canonicalize()?;
    if !root.is_dir() {
        return Err(io::Error::other("root must be a directory"));
    }
    let mut v = Volume {
        root: format!("{}\\", root.display()),
        journal: 0,
        next_usn: 0,
        entries: vec![Entry {
            id: 0,
            parent: 0,
            name: String::new(),
            directory: true,
        }],
        warnings: Vec::new(),
    };
    let mut stack = vec![(root, 0)];
    while let Some((path, parent)) = stack.pop() {
        let dir = match std::fs::read_dir(&path) {
            Ok(d) => d,
            Err(e) => {
                v.warnings.push(format!("{}: {e}", path.display()));
                continue;
            }
        };
        for item in dir {
            let item = match item {
                Ok(x) => x,
                Err(e) => {
                    v.warnings.push(e.to_string());
                    continue;
                }
            };
            let ty = match item.file_type() {
                Ok(t) => t,
                Err(e) => {
                    v.warnings.push(e.to_string());
                    continue;
                }
            };
            let id = v.entries.len() as u64;
            let name = match item.file_name().into_string() {
                Ok(n) => n,
                Err(_) => {
                    v.warnings
                        .push(format!("non-Unicode name: {}", item.path().display()));
                    continue;
                }
            };
            v.entries.push(Entry {
                id,
                parent,
                name,
                directory: ty.is_dir(),
            });
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                item.path()
                    .symlink_metadata()
                    .map(|m| m.file_attributes() & 0x400 != 0)
                    .unwrap_or(true)
            };
            #[cfg(not(windows))]
            let reparse = ty.is_symlink();
            if ty.is_dir() && !reparse {
                stack.push((item.path(), id));
            }
        }
    }
    Ok(v)
}
