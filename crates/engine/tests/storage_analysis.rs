use hypersearch_engine::{
    filesystem,
    storage_analysis::{self, Format, Options},
};
use std::sync::atomic::AtomicBool;
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
    o.exclusions.push(root.join("nested"));
    let summary =
        storage_analysis::export(std::slice::from_ref(&v), &o, &csv, &stop, |_, _| {}).unwrap();
    assert_eq!(summary.totals.logical_bytes, 100);
    assert!(summary.excluded > 0);
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
