use hypersearch_engine::{
    filesystem,
    storage_analysis::{self, Format, Options},
};
use std::sync::atomic::AtomicBool;
#[test]
fn hardlinks_long_paths_missing_metadata_and_mid_scan_cancel() {
    let temp = std::env::temp_dir().join(format!("hs-storage-edge-{}", std::process::id()));
    let root = temp.join("fixture");
    std::fs::create_dir_all(root.join("Windows")).unwrap();
    let mut deep = root.clone();
    let mut directories = vec![root.clone()];
    for i in 0..5 {
        deep = deep.join(format!("{i}_{}", "длинный_каталог_".repeat(4)));
        std::fs::create_dir(&deep).unwrap();
        directories.push(deep.clone());
    }
    let long = deep.join("秘密.txt");
    std::fs::write(&long, [0; 100]).unwrap();
    std::fs::write(root.join("original.bin"), [0; 17]).unwrap();
    std::fs::hard_link(root.join("original.bin"), root.join("alias.bin")).unwrap();
    std::fs::write(root.join("gone.bin"), [0; 11]).unwrap();
    let v = filesystem::walk(&root).unwrap();
    std::fs::remove_file(root.join("gone.bin")).unwrap();
    let output = temp.join("edges.json");
    let summary = storage_analysis::export(
        &[v],
        &Options::safe(root.clone(), Format::Json),
        &output,
        &AtomicBool::new(false),
        |_, _| {},
    )
    .unwrap();
    assert_eq!(
        summary.totals.logical_bytes, 134,
        "hard links counted per pathname, not as unique physical bytes"
    );
    assert_eq!(summary.inaccessible, 1);
    assert!(summary.method.contains("Hard links"));
    assert!(
        summary
            .top_directories
            .iter()
            .any(|d| d.path.ends_with("Windows") && d.caution.is_some())
    );
    let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
    assert!(json["directories"].as_array().unwrap().len() >= 6);
    let mut files = vec![long, root.join("original.bin"), root.join("alias.bin")];
    for i in 0..300 {
        let path = root.join(format!("cancel_{i}.txt"));
        std::fs::write(&path, b"x").unwrap();
        files.push(path);
    }
    let v = filesystem::walk(&root).unwrap();
    let cancel = AtomicBool::new(false);
    let cancelled = temp.join("mid-scan.json");
    let summary = storage_analysis::export(
        &[v],
        &Options::safe(root.clone(), Format::Json),
        &cancelled,
        &cancel,
        |done, _| {
            if done >= 256 {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
    )
    .unwrap();
    assert!(summary.cancelled);
    assert!(summary.totals.files < 303);
    let _: serde_json::Value = serde_json::from_slice(&std::fs::read(&cancelled).unwrap()).unwrap();
    for path in files {
        std::fs::remove_file(path).unwrap();
    }
    std::fs::remove_file(output).unwrap();
    std::fs::remove_file(cancelled).unwrap();
    std::fs::remove_dir(root.join("Windows")).unwrap();
    for path in directories.into_iter().rev() {
        std::fs::remove_dir(path).unwrap();
    }
    std::fs::remove_dir(temp).unwrap();
}
#[test]
fn exports_sizes_privacy_formats_and_cancel() {
    let temp = std::env::temp_dir().join(format!("hs-storage-{}", std::process::id()));
    let root = temp.join("Users/PrivateName/Documents");
    std::fs::create_dir_all(root.join("nested")).unwrap();
    std::fs::write(root.join("Отчёт.txt"), [0; 100]).unwrap();
    std::fs::write(root.join("nested/=formula.csv"), [0; 200]).unwrap();
    let v = filesystem::walk(&root).unwrap();
    let stop = AtomicBool::new(false);
    let json = temp.join("report.json");
    let o = Options::safe(root.clone(), Format::Json);
    let summary =
        storage_analysis::export(std::slice::from_ref(&v), &o, &json, &stop, |_, _| {}).unwrap();
    assert_eq!(summary.totals.logical_bytes, 300);
    assert_eq!(summary.totals.files, 2);
    assert_eq!(summary.totals.folders, 2);
    let contents = std::fs::read_to_string(&json).unwrap();
    assert!(!contents.contains("PrivateName"));
    let data: serde_json::Value = serde_json::from_str(&contents).unwrap();
    assert_eq!(data["summary"]["totals"]["logical_bytes"], 300);
    assert!(data["objects"][1]["allocated_bytes"].is_null());
    assert!(
        storage_analysis::export(std::slice::from_ref(&v), &o, &json, &stop, |_, _| {}).is_err()
    );
    let csv = temp.join("report.csv");
    let mut o = Options::safe(root.clone(), Format::Csv);
    o.exclusions.push(root.join("nested/../nested"));
    let summary =
        storage_analysis::export(std::slice::from_ref(&v), &o, &csv, &stop, |_, _| {}).unwrap();
    assert_eq!(summary.totals.logical_bytes, 100);
    assert!(summary.excluded > 0);
    let mut stale = v.clone();
    let parent = stale.entries.iter().find(|e| e.id == e.parent).unwrap().id;
    stale.entries.push(hypersearch_engine::Entry {
        id: 1000001,
        parent,
        name: "PrivateMissing".into(),
        directory: true,
    });
    stale.entries.push(hypersearch_engine::Entry {
        id: 1000002,
        parent: 1000001,
        name: "secret.txt".into(),
        directory: false,
    });
    let stale_out = temp.join("stale-excluded.json");
    let mut stale_options = Options::safe(root.clone(), Format::Json);
    stale_options
        .exclusions
        .push(root.join("nested/../PrivateMissing"));
    let summary =
        storage_analysis::export(&[stale], &stale_options, &stale_out, &stop, |_, _| {}).unwrap();
    assert_eq!(summary.excluded, 2);
    assert!(
        !std::fs::read_to_string(&stale_out)
            .unwrap()
            .contains("PrivateMissing")
    );
    std::fs::remove_file(stale_out).unwrap();
    let csv_formula = temp.join("formula.csv");
    o.exclusions.clear();
    storage_analysis::export(std::slice::from_ref(&v), &o, &csv_formula, &stop, |_, _| {}).unwrap();
    assert!(
        std::fs::read_to_string(&csv_formula)
            .unwrap()
            .contains("\"'=formula.csv\"")
    );
    let md = temp.join("report.md");
    let mut o = Options::safe(root.clone(), Format::Markdown);
    o.compact = true;
    o.neutral_names = true;
    let summary =
        storage_analysis::export(std::slice::from_ref(&v), &o, &md, &stop, |_, _| {}).unwrap();
    assert_eq!(summary.exported, 0);
    assert!(summary.omitted > 0);
    let text = std::fs::read_to_string(&md).unwrap();
    assert!(!text.contains("Отчёт.txt"));
    assert!(text.contains("OBJECT_"));
    let split = temp.join("split.json");
    let mut options = Options::safe(root.clone(), Format::Json);
    options.part_rows = 1;
    let summary =
        storage_analysis::export(std::slice::from_ref(&v), &options, &split, &stop, |_, _| {})
            .unwrap();
    assert!(summary.parts.len() > 1);
    let mut exported_objects = 0;
    for part in &summary.parts {
        let path = temp.join(part);
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        exported_objects += json["objects"].as_array().unwrap().len();
        std::fs::remove_file(path).unwrap();
    }
    assert_eq!(exported_objects, summary.exported as usize);
    let cancelled = temp.join("cancelled.json");
    let summary = storage_analysis::export(
        &[v],
        &Options::safe(root.clone(), Format::Json),
        &cancelled,
        &AtomicBool::new(true),
        |_, _| {},
    )
    .unwrap();
    assert!(summary.cancelled);
    let _: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&cancelled).unwrap()).unwrap();
    for path in [
        json,
        csv,
        csv_formula,
        md,
        cancelled,
        root.join("Отчёт.txt"),
        root.join("nested/=formula.csv"),
    ] {
        std::fs::remove_file(path).unwrap();
    }
    for path in [
        root.join("nested"),
        root.clone(),
        root.parent().unwrap().to_path_buf(),
        root.parent().unwrap().parent().unwrap().to_path_buf(),
        temp,
    ] {
        std::fs::remove_dir(path).unwrap();
    }
}
