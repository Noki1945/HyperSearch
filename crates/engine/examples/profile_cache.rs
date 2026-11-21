use hypersearch_engine::{
    search::{MemoryMode, Mode, Query, SearchIndex, cache_path},
    state_files, storage,
};
use std::{path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = PathBuf::from(std::env::args_os().nth(1).ok_or("index path required")?);
    let mode = MemoryMode::Fast;
    let cache = cache_path(&path, mode);
    let start = Instant::now();
    let mut volumes = storage::load(&path)?;
    state_files::exclude(&mut volumes, &path);
    let load = start.elapsed();
    let table_start = Instant::now();
    let (index, report) = SearchIndex::with_disk_cache(volumes, mode, &cache);
    let table = table_start.elapsed();
    println!(
        "cache_hit={}; saved={}; snapshot_load_and_filter_ms={:.3}; tables_ms={:.3}; total_ready_ms={:.3}; entries={}; cache_bytes={}; process={:?}",
        report.hit,
        report.saved,
        load.as_secs_f64() * 1000.,
        table.as_secs_f64() * 1000.,
        start.elapsed().as_secs_f64() * 1000.,
        index.len(),
        std::fs::metadata(&cache)?.len(),
        hypersearch_engine::metrics::process()
    );
    let q = Query {
        text: "excel.exe",
        mode: Mode::Substring,
        extension: Some("exe"),
        path: None,
    };
    let start = Instant::now();
    let page = index.search(&q, None, 100);
    let paths: Vec<_> = page.ids.iter().map(|&id| index.path(id)).collect();
    println!(
        "excel_results={}; search_with_paths_ms={:.6}; paths={paths:?}",
        paths.len(),
        start.elapsed().as_secs_f64() * 1000.
    );
    println!("{}", report.detail);
    Ok(())
}
