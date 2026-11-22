//! Live USN validation, confined to a uniquely named temporary fixture.
use hypersearch_engine::{
    monitor, ntfs,
    search::{Mode, Query, SearchIndex},
};
use std::{
    fmt::Write as _,
    fs, io,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
fn hits(index: &SearchIndex, name: &str) -> Vec<String> {
    index
        .search(
            &Query {
                text: name,
                mode: Mode::Exact,
                extension: None,
                path: None,
            },
            None,
            10,
        )
        .ids
        .into_iter()
        .filter_map(|id| index.path(id))
        .collect()
}
fn wait(
    index: &mut SearchIndex,
    name: &str,
    suffix: Option<&str>,
    label: &str,
    report: &mut String,
) -> io::Result<()> {
    let start = Instant::now();
    loop {
        monitor::refresh(index)?;
        let paths = hits(index, name);
        if match suffix {
            Some(s) => paths.len() == 1 && paths[0].ends_with(s),
            None => paths.is_empty(),
        } {
            break;
        }
        if start.elapsed().as_secs() > 10 {
            return Err(io::Error::other(format!(
                "{label}: journal visibility timeout"
            )));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
    writeln!(
        report,
        "{label}=PASS; incremental_ms={:.3}",
        start.elapsed().as_secs_f64() * 1000.
    )
    .unwrap();
    Ok(())
}
fn run(root: &str, report: &mut String) -> io::Result<()> {
    ntfs::VolumeHandle::open(root)?.journal()?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let fixture = std::env::temp_dir().join(format!("hypersearch-incremental-{stamp}"));
    fs::create_dir(&fixture)?;
    fs::create_dir(fixture.join("before"))?;
    fs::create_dir(fixture.join("after"))?;
    let start = Instant::now();
    let volume = ntfs::enumerate(root)?;
    let mut index = SearchIndex::new(vec![volume]);
    writeln!(
        report,
        "initial_entries={}; initial_scan_and_build_ms={:.3}",
        index.len(),
        start.elapsed().as_secs_f64() * 1000.
    )
    .unwrap();
    let name = format!("hs-live-{stamp}-Отчёт.txt");
    let renamed = format!("hs-live-{stamp}-Renamed.txt");
    fs::write(fixture.join("before").join(&name), b"test fixture")?;
    wait(
        &mut index,
        &name,
        Some(&format!("before\\{name}")),
        "create",
        report,
    )?;
    fs::rename(
        fixture.join("before").join(&name),
        fixture.join("before").join(&renamed),
    )?;
    wait(
        &mut index,
        &renamed,
        Some(&format!("before\\{renamed}")),
        "rename",
        report,
    )?;
    if !hits(&index, &name).is_empty() {
        return Err(io::Error::other("old name remains searchable"));
    }
    fs::rename(
        fixture.join("before").join(&renamed),
        fixture.join("after").join(&renamed),
    )?;
    wait(
        &mut index,
        &renamed,
        Some(&format!("after\\{renamed}")),
        "move",
        report,
    )?;
    fs::rename(fixture.join("after"), fixture.join("parent-renamed"))?;
    wait(
        &mut index,
        &renamed,
        Some(&format!("parent-renamed\\{renamed}")),
        "parent_rename",
        report,
    )?;
    fs::remove_file(fixture.join("parent-renamed").join(&renamed))?;
    wait(&mut index, &renamed, None, "delete", report)?;
    fs::remove_dir(fixture.join("before"))?;
    fs::remove_dir(fixture.join("parent-renamed"))?;
    fs::remove_dir(&fixture)?;
    writeln!(
        report,
        "user_files_modified=false; fixture_cleaned=true; full_rebuilds_after_initial=0"
    )
    .unwrap();
    Ok(())
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let arg = |key: &str| args.windows(2).find(|w| w[0] == key).map(|w| w[1].as_str());
    let mut report = String::new();
    let result = run(arg("--root").unwrap_or("C:\\"), &mut report);
    writeln!(
        report,
        "overall={}; error={:?}",
        if result.is_ok() { "PASS" } else { "FAIL" },
        result.as_ref().err()
    )
    .unwrap();
    if let Some(path) = arg("--report") {
        fs::write(path, &report).unwrap();
    }
    print!("{report}");
    if result.is_err() {
        std::process::exit(1);
    }
}
