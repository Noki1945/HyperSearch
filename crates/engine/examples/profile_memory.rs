use hypersearch_engine::{
    planning,
    search::{MemoryMode, Mode, Query, SearchIndex},
    storage,
};
use std::{hint::black_box, path::PathBuf, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let path = PathBuf::from(args.get(1).ok_or("index path required")?);
    let mode = MemoryMode::parse(args.get(2).ok_or("economy|balanced|fast required")?)
        .ok_or("invalid memory mode")?;
    let summary = planning::analyze_index(&path)?;
    println!(
        "prediction_bytes={}; mode={mode:?}",
        summary.estimate_bytes(mode)
    );
    let start = Instant::now();
    let volumes = storage::load(&path)?;
    println!("load_ms={:.3}", start.elapsed().as_secs_f64() * 1000.);
    let start = Instant::now();
    let index = SearchIndex::with_mode(volumes, mode);
    println!(
        "entries={}; build_ms={:.3}; owned_allocation_model_bytes={}; process={:?}",
        index.len(),
        start.elapsed().as_secs_f64() * 1000.,
        index.allocation_bytes(),
        hypersearch_engine::metrics::process()
    );
    for (text, mode) in [
        ("excel.exe", Mode::Exact),
        ("excel.exe", Mode::Substring),
        ("report", Mode::Substring),
        ("report 2026", Mode::Substring),
        ("zz", Mode::Substring),
        ("q", Mode::Substring),
        ("hs-missing-qzx", Mode::Substring),
        ("f", Mode::Prefix),
        ("отчёт", Mode::Substring),
    ] {
        let q = Query {
            text,
            mode,
            extension: None,
            path: None,
        };
        let normalized = text.to_lowercase();
        let expected: Vec<_> = (0..index.len() as u32)
            .filter(|&id| {
                let n = index.name(id).to_lowercase();
                match mode {
                    Mode::Exact => n == normalized,
                    Mode::Prefix => n.starts_with(&normalized),
                    Mode::Substring => normalized.split_whitespace().all(|t| n.contains(t)),
                }
            })
            .take(100)
            .collect();
        let actual = index.search(&q, None, 100);
        assert_eq!(actual.ids, expected, "{text}");
        let mut samples = Vec::new();
        for _ in 0..50 {
            let start = Instant::now();
            let page = index.search(&q, None, 100);
            black_box(
                page.ids
                    .iter()
                    .map(|&id| index.path(id))
                    .collect::<Vec<_>>(),
            );
            samples.push(start.elapsed().as_secs_f64() * 1000.);
        }
        samples.sort_by(f64::total_cmp);
        println!(
            "{mode:?} {text}: p50/p95/p99_ms={:.4}/{:.4}/{:.4}; candidates={}; target200ms={}",
            samples[24],
            samples[47],
            samples[49],
            actual.examined,
            samples[49] < 200.
        );
    }
    Ok(())
}
