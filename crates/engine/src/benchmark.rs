use crate::{
    Entry, Volume,
    search::{Mode, Query, SearchIndex},
};
use std::{hint::black_box, io, time::Instant};
pub fn synthetic(size: usize) -> Volume {
    let mut entries = Vec::with_capacity(size + 1);
    entries.push(Entry {
        id: 0,
        parent: 0,
        name: String::new(),
        directory: true,
    });
    for i in 0..size {
        entries.push(Entry {
            id: i as u64 + 1,
            parent: 0,
            name: match i % 4 {
                0 => format!("Financial_Report_2026_{i:08}.xlsx"),
                1 => format!("Фото_отпуск_{i:08}.jpg"),
                2 => format!("source_module_{i:08}.rs"),
                _ => format!("document_{i:08}.pdf"),
            },
            directory: false,
        });
    }
    Volume {
        root: "C:\\bench\\".into(),
        journal: 0,
        next_usn: 0,
        entries,
        warnings: vec![],
    }
}
fn percentiles(mut samples: Vec<f64>) -> (f64, f64, f64) {
    samples.sort_by(f64::total_cmp);
    let p = |x: f64| samples[((samples.len() - 1) as f64 * x).ceil() as usize];
    (p(0.5), p(0.95), p(0.99))
}
pub fn run(size: usize, iterations: usize) -> io::Result<()> {
    if size == 0 || size > 10_000_000 || iterations == 0 {
        return Err(io::Error::other(
            "size must be 1..10000000 and iterations > 0",
        ));
    }
    let generated = synthetic(size);
    let start = Instant::now();
    let index = SearchIndex::new(vec![generated]);
    println!(
        "entries={size}; build_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    println!("process_after_build={:?}", crate::metrics::process());
    let tmp =
        std::env::temp_dir().join(format!("hypersearch-benchmark-{}.hsi", std::process::id()));
    let start = Instant::now();
    crate::storage::save(&tmp, &index.volumes)?;
    println!(
        "save_ms={:.3}; disk_bytes={}",
        start.elapsed().as_secs_f64() * 1000.,
        std::fs::metadata(&tmp)?.len()
    );
    let start = Instant::now();
    let loaded = crate::storage::load(&tmp)?;
    println!(
        "load_os_cache_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    );
    drop(loaded);
    std::fs::remove_file(tmp)?;
    for (mode, text) in [
        (Mode::Exact, "Financial_Report_2026_00000000.xlsx"),
        (Mode::Prefix, "financial"),
        (Mode::Substring, "report"),
        (Mode::Substring, "report 2026"),
        (Mode::Substring, "отпуск"),
        (Mode::Substring, "00009999"),
        (Mode::Substring, "absent-token"),
        (Mode::Substring, "zz"),
        (Mode::Substring, "q"),
        (Mode::Substring, "fi"),
        (Mode::Prefix, "f"),
    ] {
        let q = Query {
            text,
            mode,
            extension: None,
            path: None,
        };
        let start = Instant::now();
        let first = index.search(&q, None, 100);
        println!(
            "{mode:?} {text}: first_query_ms={:.4}",
            start.elapsed().as_secs_f64() * 1000.
        );
        let normalized = text.to_lowercase();
        let tokens: Vec<_> = normalized.split_whitespace().collect();
        let matches = |n: &str| match mode {
            Mode::Exact => n == normalized,
            Mode::Prefix => n.starts_with(&normalized),
            Mode::Substring => tokens.iter().all(|t| n.contains(t)),
        };
        let reference: Vec<_> = index
            .names
            .iter()
            .enumerate()
            .filter(|(_, n)| matches(n))
            .take(100)
            .map(|(i, _)| i as u32)
            .collect();
        if first.ids != reference {
            return Err(io::Error::other("indexed search differs from reference"));
        }
        let mut samples = Vec::with_capacity(iterations);
        let mut scans = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let start = Instant::now();
            black_box(index.search(black_box(&q), None, 100));
            samples.push(start.elapsed().as_secs_f64() * 1000.);
            let start = Instant::now();
            black_box(index.names.iter().filter(|n| matches(n)).take(100).count());
            scans.push(start.elapsed().as_secs_f64() * 1000.);
        }
        println!(
            "  indexed p50/p95/p99_ms={:?}; linear={:?}; examined={}",
            percentiles(samples),
            percentiles(scans),
            first.examined
        );
    }
    let mut volume = synthetic(100_000.min(size));
    let start = Instant::now();
    let changes = (1..=1000.min(size))
        .map(|i| {
            (
                Entry {
                    id: i as u64,
                    parent: 0,
                    name: format!("renamed_{i}.txt"),
                    directory: false,
                },
                0x2000,
            )
        })
        .collect();
    crate::monitor::apply(&mut volume, changes);
    println!(
        "batch_rename_ms={:.3}; process_end={:?}",
        start.elapsed().as_secs_f64() * 1000.,
        crate::metrics::process()
    );
    Ok(())
}
