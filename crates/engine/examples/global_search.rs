//! Real-machine global filename validation. Requires elevation for the NTFS fast path.
use hypersearch_engine::{
    filesystem, indexer,
    search::{Mode, Query, SearchIndex},
    storage,
};
use std::{fmt::Write as _, fs, io, path::PathBuf, time::Instant};
fn run(out: &mut String) -> io::Result<()> {
    let app = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("HyperSearch");
    fs::create_dir_all(&app)?;
    let roots = filesystem::drives();
    if roots.is_empty() {
        return Err(io::Error::other("no accessible drives"));
    }
    writeln!(
        out,
        "scope=all accessible drive-letter roots; roots={}",
        roots.join(", ")
    )
    .unwrap();
    let total = Instant::now();
    let mut volumes = Vec::new();
    for root in roots {
        let start = Instant::now();
        let v = indexer::index(&root, false)?;
        writeln!(
            out,
            "volume={}; records={}; source={}; scan_ms={:.3}; warnings={}",
            v.root,
            v.entries.len(),
            if v.journal == 0 {
                "directory snapshot"
            } else {
                "NTFS USN"
            },
            start.elapsed().as_secs_f64() * 1000.,
            v.warnings.len()
        )
        .unwrap();
        volumes.push(v);
    }
    let start = Instant::now();
    storage::save(&app.join("index.hsi"), &volumes)?;
    writeln!(out, "save_ms={:.3}", start.elapsed().as_secs_f64() * 1000.).unwrap();
    let start = Instant::now();
    let index = SearchIndex::new(volumes);
    writeln!(
        out,
        "indexed_objects={}; tables_ms={:.3}; total_prepare_ms={:.3}",
        index.len(),
        start.elapsed().as_secs_f64() * 1000.,
        total.elapsed().as_secs_f64() * 1000.
    )
    .unwrap();
    for mode in [Mode::Exact, Mode::Substring] {
        let q = Query {
            text: "excel.exe",
            mode,
            extension: Some("exe"),
            path: None,
        };
        let start = Instant::now();
        let page = index.search(&q, None, usize::MAX);
        let paths: Vec<_> = page
            .ids
            .iter()
            .map(|&id| {
                index
                    .path(id)
                    .ok_or_else(|| io::Error::other("unresolved result"))
            })
            .collect::<io::Result<_>>()?;
        writeln!(
            out,
            "query=excel.exe; mode={mode:?}; results={}; first_with_paths_ms={:.6}; examined={}",
            paths.len(),
            start.elapsed().as_secs_f64() * 1000.,
            page.examined
        )
        .unwrap();
        for p in &paths {
            writeln!(
                out,
                "result={p}; exists={}",
                std::path::Path::new(p).is_file()
            )
            .unwrap();
        }
        if paths.iter().any(|p| !std::path::Path::new(p).is_file()) {
            return Err(io::Error::other("result no longer exists"));
        }
        let mut samples = Vec::with_capacity(100);
        for _ in 0..100 {
            let start = Instant::now();
            let p = index.search(&q, None, usize::MAX);
            std::hint::black_box(p.ids.iter().map(|&id| index.path(id)).collect::<Vec<_>>());
            samples.push(start.elapsed().as_secs_f64() * 1000.);
        }
        samples.sort_by(f64::total_cmp);
        writeln!(
            out,
            "warm_with_paths_p50_ms={:.6}; p95_ms={:.6}; p99_ms={:.6}",
            samples[49], samples[94], samples[98]
        )
        .unwrap();
    }
    writeln!(out, "saved_index={}", app.join("index.hsi").display()).unwrap();
    Ok(())
}
fn main() {
    let report = std::env::args_os().nth(1).map(PathBuf::from);
    let mut out = String::new();
    let result = run(&mut out);
    match &result {
        Ok(()) => writeln!(out, "overall=PASS").unwrap(),
        Err(e) => writeln!(out, "overall=FAIL; error={e}").unwrap(),
    };
    if let Some(path) = report {
        fs::write(path, &out).expect("write report");
    }
    print!("{out}");
    if result.is_err() {
        std::process::exit(1);
    }
}
