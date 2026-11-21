use crate::Volume;
use std::{io, path::Path};

pub fn index(root: &str, force_walk: bool) -> io::Result<Volume> {
    #[cfg(windows)]
    if !force_walk && root.len() == 3 && root.as_bytes()[1] == b':' {
        match crate::ntfs::enumerate(root) {
            Ok(v) => return Ok(v),
            Err(e) => eprintln!("NTFS fast path unavailable for {root}: {e}; using directory scan"),
        }
    }
    let _ = force_walk;
    crate::filesystem::walk(Path::new(root))
}
