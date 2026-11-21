use hypersearch_engine::{
    Volume, filesystem, monitor, ntfs,
    search::{Mode, Query, SearchIndex},
    storage,
};
use std::{
    fmt::Write as _,
    fs, io,
    path::Path,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
fn check(v: &Volume, name: &str, suffix: Option<&str>) -> io::Result<()> {
    let lookup = v.paths();
    let hits: Vec<_> = v.entries.iter().filter(|e| e.name == name).collect();
    match suffix {
        None if hits.is_empty() => Ok(()),
        Some(s)
            if hits.len() == 1 && v.path(hits[0].id, &lookup).is_some_and(|p| p.ends_with(s)) =>
        {
            Ok(())
        }
        _ => Err(io::Error::other(format!(
            "unexpected state for {name}: {} records",
            hits.len()
        ))),
    }
}
fn live(root: &str, out: &mut String) -> io::Result<()> {
    let handle = ntfs::VolumeHandle::open(root)?;
    let metadata = handle.journal()?;
    writeln!(
        out,
        "journal_open=PASS; journal_id={}; first={}; next={}",
        metadata.0, metadata.1, metadata.2
    )
    .unwrap();
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("hypersearch-usn-{stamp}"));
    fs::create_dir(&dir)?;
    fs::create_dir(dir.join("before"))?;
    fs::create_dir(dir.join("after"))?;
    let start = Instant::now();
    let mut volume = ntfs::enumerate(root)?;
    writeln!(
        out,
        "enumeration=PASS; records={}; milliseconds={:.3}",
        volume.entries.len(),
        start.elapsed().as_secs_f64() * 1000.
    )
    .unwrap();
    let name = format!("hs-usn-{stamp}-Отчёт.txt");
    let renamed = format!("hs-usn-{stamp}-Renamed.txt");
    let poll = |v: &mut Volume, label: &str, out: &mut String| -> io::Result<()> {
        let start = Instant::now();
        monitor::catch_up(v)?;
        writeln!(
            out,
            "{label}_replay_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        )
        .unwrap();
        Ok(())
    };
    fs::write(dir.join("before").join(&name), b"fixture")?;
    poll(&mut volume, "create", out)?;
    check(&volume, &name, Some(&format!("before\\{name}")))?;
    writeln!(out, "create=PASS").unwrap();
    fs::rename(
        dir.join("before").join(&name),
        dir.join("before").join(&renamed),
    )?;
    poll(&mut volume, "rename", out)?;
    check(&volume, &name, None)?;
    check(&volume, &renamed, Some(&format!("before\\{renamed}")))?;
    writeln!(out, "rename=PASS").unwrap();
    fs::rename(
        dir.join("before").join(&renamed),
        dir.join("after").join(&renamed),
    )?;
    poll(&mut volume, "move", out)?;
    check(&volume, &renamed, Some(&format!("after\\{renamed}")))?;
    writeln!(out, "move=PASS").unwrap();
    fs::rename(dir.join("after"), dir.join("moved-parent"))?;
    poll(&mut volume, "parent_rename", out)?;
    check(&volume, &renamed, Some(&format!("moved-parent\\{renamed}")))?;
    writeln!(out, "parent_rename=PASS").unwrap();
    let snapshot = dir.join("snapshot.hsi");
    storage::save(&snapshot, &[volume])?;
    let mut volume = storage::load(&snapshot)?.remove(0);
    fs::remove_file(&snapshot)?;
    fs::remove_file(dir.join("moved-parent").join(&renamed))?;
    poll(&mut volume, "delete_after_resume", out)?;
    check(&volume, &renamed, None)?;
    writeln!(out, "checkpoint_resume_and_delete=PASS").unwrap();
    let mut bad = volume.clone();
    bad.journal ^= 1;
    if monitor::catch_up(&mut bad).is_ok() {
        return Err(io::Error::other("journal identity mismatch accepted"));
    }
    writeln!(out, "journal_identity_mismatch=PASS").unwrap();
    let mut bad = volume.clone();
    bad.next_usn = -1;
    if monitor::catch_up(&mut bad).is_ok() {
        return Err(io::Error::other("expired checkpoint accepted"));
    }
    writeln!(out, "expired_checkpoint=PASS").unwrap();
    fs::remove_dir(dir.join("before"))?;
    fs::remove_dir(dir.join("moved-parent"))?;
    fs::remove_dir(&dir)?;
    poll(&mut volume, "directory_delete", out)?;
    storage::verify(&volume)?;
    writeln!(out, "final_graph=PASS").unwrap();
    Ok(())
}
fn dataset(root: &Path, out: &mut String) -> io::Result<()> {
    fs::create_dir_all(root)?;
    let start = Instant::now();
    for d in 0..100 {
        let dir = root.join(format!("folder_{d:03}"));
        fs::create_dir_all(&dir)?;
        for f in 0..200 {
            let name = if f == 0 {
                format!("hsneedle_report_{d:03}.txt")
            } else if f == 1 {
                format!("Отчёт_2026_{d:03}.txt")
            } else {
                format!("document_{d:03}_{f:03}.txt")
            };
            fs::write(dir.join(name), b"HyperSearch validation fixture\n")?;
        }
    }
    writeln!(
        out,
        "fixture={}; files=20000; generation_ms={:.3}",
        root.display(),
        start.elapsed().as_secs_f64() * 1000.
    )
    .unwrap();
    let start = Instant::now();
    let volume = filesystem::walk(root)?;
    writeln!(
        out,
        "walk_ms={:.3}; warnings={}",
        start.elapsed().as_secs_f64() * 1000.,
        volume.warnings.len()
    )
    .unwrap();
    let start = Instant::now();
    let index = SearchIndex::new(vec![volume]);
    writeln!(
        out,
        "search_tables_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    )
    .unwrap();
    for (text, expected) in [("hsneedle", 100), ("отчёт", 100), ("hsabsent", 0)] {
        let q = Query {
            text,
            mode: Mode::Substring,
            extension: None,
            path: None,
        };
        let start = Instant::now();
        let p = index.search(&q, None, usize::MAX);
        let paths: Vec<_> = p.ids.iter().map(|&id| index.path(id).unwrap()).collect();
        writeln!(
            out,
            "query={text}; count={}; first_query_with_paths_ms={:.4}",
            paths.len(),
            start.elapsed().as_secs_f64() * 1000.
        )
        .unwrap();
        if paths.len() != expected {
            return Err(io::Error::other("unexpected fixture results"));
        }
        let mut samples = Vec::new();
        for _ in 0..100 {
            let start = Instant::now();
            let p = index.search(&q, None, usize::MAX);
            std::hint::black_box(
                p.ids
                    .iter()
                    .map(|&id| index.path(id).unwrap())
                    .collect::<Vec<_>>(),
            );
            samples.push(start.elapsed().as_secs_f64() * 1000.);
        }
        samples.sort_by(f64::total_cmp);
        writeln!(
            out,
            "warm_with_paths_p50_ms={:.4}; p95_ms={:.4}; p99_ms={:.4}",
            samples[49], samples[94], samples[98]
        )
        .unwrap();
    }
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let arg = |key: &str| args.windows(2).find(|w| w[0] == key).map(|w| w[1].as_str());
    let mut out = String::new();
    let result = if let Some(root) = arg("--ntfs") {
        live(root, &mut out)
    } else if let Some(root) = arg("--dataset") {
        dataset(Path::new(root), &mut out)
    } else {
        Err(io::Error::other(
            "--ntfs C:\\ OR --dataset PATH; --report PATH",
        ))
    };
    match &result {
        Ok(()) => writeln!(out, "overall=PASS").unwrap(),
        Err(e) => writeln!(out, "overall=FAIL; error={e}").unwrap(),
    };
    if let Some(report) = arg("--report") {
        fs::write(report, &out).expect("write validation report");
    }
    print!("{out}");
    if result.is_err() {
        std::process::exit(1);
    }
}
