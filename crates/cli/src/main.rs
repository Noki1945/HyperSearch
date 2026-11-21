use hypersearch_engine::{
    filesystem, indexer, monitor, planning,
    search::{MemoryMode, Mode, Query, SearchIndex},
    storage,
};
use std::{
    io::{self, Write},
    path::PathBuf,
    time::Instant,
};

fn data_path() -> PathBuf {
    std::env::var_os("HYPERSEARCH_INDEX")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join("HyperSearch/index.hsi")
        })
}
fn option<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.windows(2).find(|w| w[0] == key).map(|w| w[1].as_str())
}
fn refresh(volumes: &mut [hypersearch_engine::Volume]) -> io::Result<()> {
    for v in volumes {
        if v.journal == 0 {
            eprintln!("Snapshot only (not live): {}", v.root);
            continue;
        }
        if let Err(e) = monitor::catch_up(v) {
            eprintln!("{}: {e}; rebuilding volume", v.root);
            *v = indexer::index(&v.root, false)?;
        }
    }
    Ok(())
}
fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("interactive");
    let path = data_path();
    match cmd {
        "index" | "rebuild" => {
            let roots = option(&args, "--root")
                .map(|s| vec![s.to_string()])
                .unwrap_or_else(filesystem::drives);
            let start = Instant::now();
            let mut volumes = Vec::new();
            for root in roots {
                let v = indexer::index(&root, args.iter().any(|s| s == "--walk"))?;
                for w in &v.warnings {
                    eprintln!("{w}");
                }
                println!("{}: {} entries", v.root, v.entries.len());
                volumes.push(v);
            }
            hypersearch_engine::state_files::exclude(&mut volumes, &path);
            storage::save(&path, &volumes)?;
            println!(
                "Saved {} in {:.3}s",
                path.display(),
                start.elapsed().as_secs_f64()
            );
        }
        "benchmark" => hypersearch_engine::benchmark::run(
            option(&args, "--size").unwrap_or("100000").parse()?,
            option(&args, "--iterations").unwrap_or("100").parse()?,
        )?,
        "estimate" => {
            let summary = planning::analyze_index(&path)?;
            println!("{}", summary.description());
            if let Some(mb) = option(&args, "--budget-mb") {
                println!(
                    "Selected profile: {} (budget is a target, not a hard cap)",
                    summary.budget_mode(mb.parse()?)?.key()
                );
            }
        }
        "verify" => {
            let v = storage::load(&path)?;
            println!("Integrity OK: {} volumes (freshness not verified)", v.len());
        }
        "search" | "stats" | "interactive" | "watch" => {
            let mut volumes = storage::load(&path)?;
            refresh(&mut volumes)?;
            hypersearch_engine::state_files::exclude(&mut volumes, &path);
            storage::save(&path, &volumes)?;
            if cmd == "watch" {
                if volumes.iter().any(|v| v.journal == 0) {
                    return Err(
                        "watch requires USN-backed volumes; directory snapshots need rebuild"
                            .into(),
                    );
                }
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    let before: Vec<_> = volumes.iter().map(|v| (v.journal, v.next_usn)).collect();
                    refresh(&mut volumes)?;
                    if volumes.iter().map(|v| (v.journal, v.next_usn)).ne(before) {
                        storage::save(&path, &volumes)?;
                    }
                }
            }
            let start = Instant::now();
            let memory = if let Some(mb) = option(&args, "--budget-mb") {
                planning::analyze_index(&path)?.budget_mode(mb.parse()?)?
            } else {
                MemoryMode::parse(option(&args, "--memory").unwrap_or("fast"))
                    .ok_or("memory must be economy, balanced or fast")?
            };
            let (index, cache) = SearchIndex::with_disk_cache(
                volumes,
                memory,
                &hypersearch_engine::search::cache_path(&path, memory),
            );
            eprintln!("{}", cache.detail);
            eprintln!(
                "Search index ready in {:.3}s",
                start.elapsed().as_secs_f64()
            );
            if cmd == "stats" {
                println!(
                    "Indexed objects: {}\nStorage: {} bytes\nVolumes: {}",
                    index.len(),
                    std::fs::metadata(&path)?.len(),
                    index.volumes.len()
                );
                println!(
                    "Memory profile: {}; allocation model: {} MiB",
                    memory.key(),
                    index.allocation_bytes() / 1024 / 1024
                );
                for v in &index.volumes {
                    println!(
                        "{}: {}",
                        v.root,
                        if v.journal == 0 {
                            "directory snapshot"
                        } else {
                            "USN caught up at command start"
                        }
                    );
                }
                return Ok(());
            }
            let mode = match option(&args, "--mode").unwrap_or("substring") {
                "exact" => Mode::Exact,
                "prefix" => Mode::Prefix,
                "substring" => Mode::Substring,
                _ => return Err("mode must be exact, prefix or substring".into()),
            };
            let limit = option(&args, "--limit").unwrap_or("100").parse()?;
            let after = option(&args, "--after").map(str::parse).transpose()?;
            let search = |text: &str| {
                let start = Instant::now();
                let page = index.search(
                    &Query {
                        text,
                        mode,
                        extension: option(&args, "--ext"),
                        path: option(&args, "--path"),
                    },
                    after,
                    limit,
                );
                let elapsed = start.elapsed();
                for id in &page.ids {
                    println!(
                        "{}",
                        index
                            .path(*id)
                            .unwrap_or_else(|| "<unresolved path>".into())
                    );
                }
                println!(
                    "Returned: {}; search: {:.3} ms; examined: {}; next cursor: {:?}",
                    page.ids.len(),
                    elapsed.as_secs_f64() * 1000.,
                    page.examined,
                    page.next
                );
            };
            if cmd == "search" {
                search(
                    args.get(1)
                        .filter(|s| !s.starts_with("--"))
                        .ok_or("missing query")?,
                );
            } else {
                println!(
                    "HyperSearch v0.1 — {} objects; snapshot session; :quit to exit",
                    index.len()
                );
                let mut line = String::new();
                loop {
                    print!("Search > ");
                    io::stdout().flush()?;
                    line.clear();
                    if io::stdin().read_line(&mut line)? == 0 || line.trim() == ":quit" {
                        break;
                    }
                    search(line.trim());
                }
            }
        }
        "help" | "--help" | "-h" => println!(
            "hypersearch index|rebuild [--root C:\\] [--walk]\nhypersearch search QUERY [--mode exact|prefix|substring] [--ext xlsx] [--path DIR] [--limit 100] [--after ID] [--memory economy|balanced|fast] [--budget-mb N]\nhypersearch estimate [--budget-mb N]\nhypersearch stats|verify|watch\nhypersearch benchmark [--size 100000|1000000|5000000|10000000] [--iterations 100]\nHYPERSEARCH_INDEX overrides the application index path."
        ),
        _ => return Err(format!("unknown command: {cmd}").into()),
    }
    Ok(())
}
fn main() {
    if let Err(e) = run() {
        eprintln!("HyperSearch: {e}");
        std::process::exit(1);
    }
}
