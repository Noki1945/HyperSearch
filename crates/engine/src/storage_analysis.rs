//! On-demand, metadata-only export of an existing snapshot. No file contents or network access.
use crate::Volume;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, BufWriter, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::UNIX_EPOCH,
};
#[derive(Clone, Copy, Debug)]
pub enum Format {
    Json,
    Csv,
    Markdown,
}
#[derive(Clone)]
pub struct Options {
    pub root: PathBuf,
    pub format: Format,
    pub safe: bool,
    pub neutral_names: bool,
    pub compact: bool,
    pub max_depth: usize,
    pub min_bytes: u64,
    pub exclusions: Vec<PathBuf>,
}
impl Options {
    pub fn safe(root: PathBuf, format: Format) -> Self {
        Self {
            root,
            format,
            safe: true,
            neutral_names: false,
            compact: false,
            max_depth: usize::MAX,
            min_bytes: 0,
            exclusions: vec![],
        }
    }
}
#[derive(Clone, Serialize, Debug)]
pub struct Object {
    pub name: String,
    pub path: String,
    pub parent: String,
    pub kind: String,
    pub extension: String,
    pub logical_bytes: Option<u64>,
    pub allocated_bytes: Option<u64>,
    pub modified_unix_seconds: Option<u64>,
    pub created_unix_seconds: Option<u64>,
    pub depth: usize,
    pub attributes: Option<u32>,
    pub reparse: bool,
    pub warning: Option<String>,
}
#[derive(Clone, Default, Serialize, Debug)]
pub struct Totals {
    pub logical_bytes: u64,
    pub files: u64,
    pub folders: u64,
}
#[derive(Clone, Serialize, Debug)]
pub struct Directory {
    pub path: String,
    pub totals: Totals,
    pub share_percent: f64,
    pub caution: Option<String>,
}
#[derive(Default, Serialize, Debug)]
pub struct Summary {
    pub root: String,
    pub totals: Totals,
    pub exported: u64,
    pub omitted: u64,
    pub excluded: u64,
    pub inaccessible: u64,
    pub reparse_skipped: u64,
    pub cancelled: bool,
    pub snapshot_warnings: usize,
    pub top_files: Vec<Object>,
    pub top_directories: Vec<Directory>,
    pub extensions: HashMap<String, Totals>,
    pub categories: HashMap<String, Totals>,
    pub volume_capacity_bytes: Option<u64>,
    pub volume_available_bytes: Option<u64>,
    pub old_large_files: Vec<Object>,
    pub review_candidates: Vec<Object>,
    pub method: String,
    pub privacy: String,
}
fn normalized(p: &Path) -> String {
    p.to_string_lossy()
        .replace('/', "\\")
        .trim_start_matches("\\\\?\\")
        .trim_end_matches('\\')
        .to_lowercase()
}
fn inside(path: &Path, root: &Path) -> bool {
    let p = normalized(path);
    let r = normalized(root);
    p == r || p.starts_with(&(r + "\\"))
}
fn redact(path: &Path, o: &Options) -> String {
    if !o.safe {
        return path.to_string_lossy().into_owned();
    }
    let text = path.to_string_lossy().replace('/', "\\");
    let mut parts: Vec<String> = text.split('\\').map(str::to_string).collect();
    for i in 0..parts.len().saturating_sub(1) {
        if parts[i].eq_ignore_ascii_case("users") {
            parts[i + 1] = "USERNAME".into();
        }
    }
    if o.neutral_names {
        use std::hash::{Hash, Hasher};
        for p in &mut parts {
            if p.is_empty() || p.ends_with(':') || p == "?" || p == "USERNAME" {
                continue;
            }
            let mut h = std::collections::hash_map::DefaultHasher::new();
            p.hash(&mut h);
            *p = format!("OBJECT_{:016x}", h.finish());
        }
    }
    parts.join("\\")
}
fn csv(s: &str) -> String {
    let prefix = if s.starts_with(['=', '+', '-', '@', '\t', '\r']) {
        "'"
    } else {
        ""
    };
    format!("\"{prefix}{}\"", s.replace('"', "\"\""))
}
fn json_error(e: serde_json::Error) -> io::Error {
    io::Error::other(e)
}
fn caution(path: &Path) -> Option<String> {
    let p = normalized(path);
    ["windows", "program files", "program files (x86)", "programdata", "appdata", "system volume information", "windows.old", "pagefile.sys", "hiberfil.sys"]
        .iter().any(|name| p.split('\\').any(|part| part == *name))
        .then(|| "Protected/application data: use Windows cleanup or the application's uninstaller; do not delete manually.".into())
}
fn category(extension: &str) -> &'static str {
    match extension {
        "doc" | "docx" | "pdf" | "xls" | "xlsx" | "txt" | "pptx" | "odt" => "documents",
        "exe" | "dll" | "msi" | "sys" => "applications",
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "svg" => "images",
        "mp3" | "flac" | "wav" | "m4a" | "ogg" => "audio",
        "mp4" | "mkv" | "avi" | "mov" | "webm" => "video",
        "zip" | "7z" | "rar" | "gz" | "tar" | "iso" => "archives",
        _ => "other",
    }
}
fn disk_space(root: &Path) -> (Option<u64>, Option<u64>) {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(
                path: *const u16,
                available: *mut u64,
                total: *mut u64,
                free: *mut u64,
            ) -> i32;
        }
        let path: Vec<u16> = root.as_os_str().encode_wide().chain(Some(0)).collect();
        let (mut available, mut total, mut free) = (0, 0, 0);
        if unsafe { GetDiskFreeSpaceExW(path.as_ptr(), &mut available, &mut total, &mut free) } != 0
        {
            return (Some(total), Some(available));
        }
    }
    let _ = root;
    (None, None)
}
fn top(files: &mut Vec<Object>, object: &Object) {
    files.push(object.clone());
    files.sort_unstable_by_key(|f| std::cmp::Reverse(f.logical_bytes));
    files.truncate(100);
}
pub fn preview(o: &Options) -> String {
    format!(
        "Корень: {}\nБезопасный режим: {}\nНейтральные имена: {}\nКомпактный: {}\nИсключений: {}\nСодержимое файлов не читается. Даже скрытые имена пользователей не гарантируют анонимность названий документов. Ничего не отправляется в интернет.",
        redact(&o.root, o),
        o.safe,
        o.neutral_names,
        o.compact,
        o.exclusions.len()
    )
}
pub const AI_PROMPT: &str = "Проанализируй приложенный отчёт о файловой системе Windows. Найди крупные каталоги и файлы — кандидаты на проверку. Раздели рекомендации на безопасные, требующие проверки и системные объекты, которые нельзя удалять вручную. Не суммируй вложенные каталоги повторно. Не предлагай удаление Windows, WinSxS, System32, Program Files, ProgramData, AppData, pagefile.sys, hiberfil.sys или System Volume Information без штатной процедуры. Для программ предложи штатную деинсталляцию; для Windows.old — штатную очистку после проверки. Объясни риски и ограничения неполных данных. Размер и возраст не доказывают ненужность или дубликаты.";
/// Memory scales with directories/extension categories and top-100, not all file metadata.
pub fn export(
    volumes: &[Volume],
    o: &Options,
    destination: &Path,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64, u64),
) -> io::Result<Summary> {
    let root = o.root.canonicalize()?;
    if !root.is_dir() {
        return Err(io::Error::other("analysis root must be a directory"));
    }
    let mut writer = BufWriter::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination)?,
    );
    let mut summary=Summary{root:redact(&root,o),snapshot_warnings:volumes.iter().map(|v|v.warnings.len()).sum(),method:"Indexed names + symlink_metadata; logical sizes counted per indexed pathname. Allocated/physical bytes unknown. Hard links may be counted more than once; USN may omit alternate names. Reparse points are not traversed. Snapshot may be stale. No hashes/confirmed duplicates. Directory totals overlap: never sum top directories. No automatic deletion.".into(),privacy:if o.safe{"Username masking enabled; document names may still be sensitive."}else{"Full paths and names included."}.into(),..Default::default()};
    match o.format {
        Format::Json => writer.write_all(b"{\"schema\":\"hypersearch-storage-1\",\"objects\":[")?,
        Format::Csv => writeln!(
            writer,
            "name,path,parent,kind,extension,logical_bytes,allocated_bytes,modified_unix_seconds,created_unix_seconds,depth,attributes,reparse,warning"
        )?,
        Format::Markdown => writeln!(
            writer,
            "# Storage report\n\n{}\n\n{}\n",
            preview(o),
            summary.method
        )?,
    }
    (
        summary.volume_capacity_bytes,
        summary.volume_available_bytes,
    ) = disk_space(&root);
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut folders: HashMap<PathBuf, Totals> = HashMap::new();
    let mut blocked_parents: HashMap<PathBuf, bool> = HashMap::new();
    let total = volumes.iter().map(|v| v.entries.len() as u64).sum();
    let mut visited = 0u64;
    'volumes: for v in volumes {
        let lookup = v.paths();
        for e in &v.entries {
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                break 'volumes;
            }
            visited += 1;
            if visited.is_multiple_of(256) {
                progress(visited, total);
            }
            let Some(path) = v.path(e.id, &lookup).map(PathBuf::from) else {
                summary.inaccessible += 1;
                continue;
            };
            if !inside(&path, &root) {
                continue;
            }
            if o.exclusions.iter().any(|p| inside(&path, p)) {
                summary.excluded += 1;
                continue;
            }
            if path == destination {
                continue;
            }
            let mut unsafe_parent = false;
            for parent in path.ancestors().skip(1) {
                if !inside(parent, &root) {
                    break;
                }
                let blocked = *blocked_parents
                    .entry(parent.to_path_buf())
                    .or_insert_with(|| match fs::symlink_metadata(parent) {
                        Ok(m) => {
                            #[cfg(windows)]
                            {
                                use std::os::windows::fs::MetadataExt;
                                m.file_attributes() & 0x400 != 0
                            }
                            #[cfg(not(windows))]
                            {
                                m.file_type().is_symlink()
                            }
                        }
                        Err(_) => true,
                    });
                if blocked {
                    unsafe_parent = true;
                    break;
                }
            }
            if unsafe_parent {
                summary.reparse_skipped += 1;
                continue;
            }
            let depth = path
                .components()
                .count()
                .saturating_sub(root.components().count());
            let metadata = fs::symlink_metadata(&path);
            let mut object = Object {
                name: String::new(),
                path: redact(&path, o),
                parent: path.parent().map(|p| redact(p, o)).unwrap_or_default(),
                kind: if e.directory { "directory" } else { "file" }.into(),
                extension: if e.directory {
                    String::new()
                } else {
                    Path::new(&e.name)
                        .extension()
                        .map(|s| s.to_string_lossy().to_lowercase())
                        .unwrap_or_default()
                },
                logical_bytes: None,
                allocated_bytes: None,
                modified_unix_seconds: None,
                created_unix_seconds: None,
                depth,
                attributes: None,
                reparse: false,
                warning: None,
            };
            object.name = if o.safe && o.neutral_names {
                object.path.rsplit('\\').next().unwrap_or("").into()
            } else {
                e.name.clone()
            };
            let mut file_bytes = 0;
            match metadata {
                Ok(m) => {
                    #[cfg(windows)]
                    {
                        use std::os::windows::fs::MetadataExt;
                        object.attributes = Some(m.file_attributes());
                        object.reparse = m.file_attributes() & 0x400 != 0;
                    }
                    #[cfg(not(windows))]
                    {
                        object.reparse = m.file_type().is_symlink();
                    }
                    if object.reparse {
                        summary.reparse_skipped += 1;
                        object.warning =
                            Some("reparse point: target not traversed or counted".into());
                    } else if !m.is_dir() {
                        file_bytes = m.len();
                        object.logical_bytes = Some(file_bytes);
                    }
                    object.modified_unix_seconds = m
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs());
                    object.created_unix_seconds = m
                        .created()
                        .ok()
                        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                        .map(|d| d.as_secs());
                }
                Err(error) => {
                    summary.inaccessible += 1;
                    object.warning = Some(format!("metadata unavailable: {:?}", error.kind()));
                }
            }
            if e.directory {
                summary.totals.folders += 1;
            } else {
                summary.totals.files += 1;
                summary.totals.logical_bytes =
                    summary.totals.logical_bytes.saturating_add(file_bytes);
                let t = summary
                    .extensions
                    .entry(object.extension.clone())
                    .or_default();
                t.files += 1;
                t.logical_bytes = t.logical_bytes.saturating_add(file_bytes);
                let c = summary
                    .categories
                    .entry(category(&object.extension).into())
                    .or_default();
                c.files += 1;
                c.logical_bytes = c.logical_bytes.saturating_add(file_bytes);
            }
            let mut parent = if e.directory {
                Some(path.clone())
            } else {
                path.parent().map(Path::to_path_buf)
            };
            while let Some(p) = parent {
                if !inside(&p, &root) {
                    break;
                }
                let t = folders.entry(p.clone()).or_default();
                t.logical_bytes = t.logical_bytes.saturating_add(file_bytes);
                if e.directory {
                    if p != path {
                        t.folders += 1;
                    }
                } else {
                    t.files += 1;
                }
                if normalized(&p) == normalized(&root) {
                    break;
                }
                parent = p.parent().map(Path::to_path_buf);
            }
            if !e.directory && file_bytes > 0 {
                top(&mut summary.top_files, &object);
                if file_bytes >= 100 * 1024 * 1024 {
                    if object
                        .modified_unix_seconds
                        .is_some_and(|t| now.saturating_sub(t) >= 90 * 86400)
                    {
                        top(&mut summary.old_large_files, &object);
                    }
                    let p = normalized(&path);
                    if caution(&path).is_none()
                        && (p.split('\\').any(|s| s == "downloads" || s == "temp")
                            || matches!(object.extension.as_str(), "bak" | "log" | "iso" | "msi"))
                    {
                        let mut candidate = object.clone();
                        candidate.warning = Some("Review only: size, age and extension do not establish that a file is disposable.".into());
                        top(&mut summary.review_candidates, &candidate);
                    }
                }
            }
            if o.compact || depth > o.max_depth || (!e.directory && file_bytes < o.min_bytes) {
                summary.omitted += 1;
                continue;
            }
            match o.format {
                Format::Json => {
                    if summary.exported > 0 {
                        writer.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut writer, &object).map_err(json_error)?;
                }
                Format::Csv => {
                    writeln!(
                        writer,
                        "{},{},{},{},{},{},,{},{},{},{},{},{}",
                        csv(&object.name),
                        csv(&object.path),
                        csv(&object.parent),
                        csv(&object.kind),
                        csv(&object.extension),
                        object
                            .logical_bytes
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        object
                            .modified_unix_seconds
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        object
                            .created_unix_seconds
                            .map(|n| n.to_string())
                            .unwrap_or_default(),
                        depth,
                        object.attributes.map(|n| n.to_string()).unwrap_or_default(),
                        object.reparse,
                        csv(object.warning.as_deref().unwrap_or(""))
                    )?;
                }
                Format::Markdown => writeln!(
                    writer,
                    "- `{}` — {} bytes{}",
                    object.path.replace('`', "'"),
                    object
                        .logical_bytes
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "unknown".into()),
                    object
                        .warning
                        .as_ref()
                        .map(|s| format!("; {s}"))
                        .unwrap_or_default()
                )?,
            }
            summary.exported += 1;
        }
    }
    for (path, totals) in folders {
        summary.top_directories.push(Directory {
            caution: caution(&path),
            path: redact(&path, o),
            share_percent: if summary.totals.logical_bytes > 0 {
                totals.logical_bytes as f64 * 100. / summary.totals.logical_bytes as f64
            } else {
                0.
            },
            totals,
        });
    }
    summary
        .top_directories
        .sort_unstable_by_key(|d| std::cmp::Reverse(d.totals.logical_bytes));
    summary.top_directories.truncate(100);
    match o.format {
        Format::Json => {
            writer.write_all(b"],\"summary\":")?;
            serde_json::to_writer(&mut writer, &summary).map_err(json_error)?;
            writer.write_all(b"}")?;
        }
        Format::Csv => {
            writeln!(
                writer,
                "{},,,{},,,,,,,,,{}",
                csv("# summary"),
                csv("summary"),
                csv(&serde_json::to_string(&summary).map_err(json_error)?)
            )?;
        }
        Format::Markdown => {
            writeln!(
                writer,
                "\n## Summary\n\nLogical bytes: {}\nFiles: {}\nDirectories: {}\nOmitted/aggregated: {}\nExcluded: {}\nUnavailable: {}\nCancelled: {}\n\n## Largest directories (overlapping totals)\n",
                summary.totals.logical_bytes,
                summary.totals.files,
                summary.totals.folders,
                summary.omitted,
                summary.excluded,
                summary.inaccessible,
                summary.cancelled
            )?;
            for d in &summary.top_directories {
                writeln!(
                    writer,
                    "- `{}` — {} bytes ({:.2}%)",
                    d.path.replace('`', "'"),
                    d.totals.logical_bytes,
                    d.share_percent
                )?;
            }
            writeln!(writer, "\n## Largest files\n")?;
            for f in &summary.top_files {
                writeln!(
                    writer,
                    "- `{}` — {} bytes",
                    f.path.replace('`', "'"),
                    f.logical_bytes.unwrap_or(0)
                )?;
            }
            writeln!(writer, "\n## Request for AI\n\n{AI_PROMPT}")?;
        }
    }
    writer.flush()?;
    writer.get_ref().sync_all()?;
    progress(visited, total);
    Ok(summary)
}
