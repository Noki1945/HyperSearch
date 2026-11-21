use crate::Volume;
use std::io;

/// Applies a complete journal batch by FRN. Renames update parent links, so descendants need no rewrite.
pub fn apply(volume: &mut Volume, changes: Vec<(crate::Entry, u32)>) {
    let mut lookup = volume.paths();
    let mut deleted = std::collections::HashSet::new();
    for (entry, reason) in changes {
        if reason & 0x200 != 0 {
            deleted.insert(entry.id);
            continue;
        }
        if reason & 0x1000 != 0 && reason & 0x2000 == 0 {
            continue;
        }
        deleted.remove(&entry.id);
        if let Some(&i) = lookup.get(&entry.id) {
            volume.entries[i] = entry;
        } else {
            lookup.insert(entry.id, volume.entries.len());
            volume.entries.push(entry);
        }
    }
    volume.entries.retain(|e| !deleted.contains(&e.id));
}
pub fn catch_up(volume: &mut Volume) -> io::Result<()> {
    if volume.journal == 0 {
        return Err(io::Error::other(
            "directory snapshot: freshness requires a new scan",
        ));
    }
    #[cfg(windows)]
    {
        let handle = crate::ntfs::VolumeHandle::open(&volume.root)?;
        let (journal, first, target) = handle.journal()?;
        if journal != volume.journal || volume.next_usn < first || volume.next_usn > target {
            return Err(io::Error::other(
                "journal gap or recreation: rebuild required",
            ));
        }
        while volume.next_usn < target {
            let (next, changes) = handle.changes(journal, volume.next_usn)?;
            if next <= volume.next_usn {
                return Err(io::Error::other("journal did not advance"));
            }
            if changes.iter().any(|(_, r)| r & 0x10000 != 0) {
                return Err(io::Error::other(
                    "hard-link change requires rebuild with --walk",
                ));
            }
            apply(volume, changes);
            volume.next_usn = next;
        }
        let (current, first, _) = handle.journal()?;
        if current != journal || volume.next_usn < first {
            return Err(io::Error::other(
                "journal changed during replay: rebuild required",
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err(io::Error::other("USN requires Windows"))
    }
}
