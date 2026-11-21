fn main() {
    #[cfg(windows)]
    match hypersearch_engine::ntfs::enumerate("C:\\") {
        Ok(v) => println!(
            "NTFS enumeration OK: {} records; journal={}; usn={}; graph={:?}",
            v.entries.len(),
            v.journal,
            v.next_usn,
            hypersearch_engine::storage::verify(&v)
        ),
        Err(e) => println!("NTFS enumeration unavailable: {e}"),
    }
}
