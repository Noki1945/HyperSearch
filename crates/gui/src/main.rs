#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod app;
#[cfg(windows)]
mod appearance;
#[cfg(windows)]
mod folders;
#[cfg(windows)]
#[allow(dead_code)]
mod glass;
#[cfg(windows)]
#[allow(dead_code)]
mod glass_shader;
#[cfg(windows)]
mod icons;
#[cfg(windows)]
mod layout;
#[cfg(windows)]
mod metadata;
#[cfg(windows)]
mod settings;
#[cfg(windows)]
mod status;
#[cfg(windows)]
mod theme;
#[cfg(windows)]
mod view_model;
#[cfg(windows)]
mod windirstat;
#[cfg(windows)]
mod windirstat_download;
fn main() {
    #[cfg(windows)]
    {
        if std::env::args().any(|arg| arg == "--test-windirstat-cancel") {
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(250));
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
            });
            let result = windirstat_download::download(&cancel);
            let cancelled = result
                .as_ref()
                .err()
                .is_some_and(|e| e.to_string().contains("отменена"));
            let output = std::env::var_os("HYPERSEARCH_UI_REPORT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| windirstat_download::directory().join("cancel-validation.txt"));
            let saved = std::fs::write(
                output,
                format!("cancelled={cancelled}; installer_executed=false\n"),
            )
            .is_ok();
            if !cancelled || !saved {
                std::process::exit(1);
            }
            return;
        }
        if std::env::args().any(|arg| arg == "--test-windirstat-download") {
            let result = windirstat_download::download(&std::sync::atomic::AtomicBool::new(false));
            let success = result.is_ok();
            let text = match result {
                Ok(p) => format!(
                    "version={}; sha256={}; cache_hit={}; installer_executed=false\n",
                    p.version, p.sha256, p.cached
                ),
                Err(e) => format!("error={e}\ninstaller_executed=false\n"),
            };
            let output = std::env::var_os("HYPERSEARCH_UI_REPORT")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| windirstat_download::directory().join("validation.txt"));
            let saved = std::fs::write(output, text).is_ok();
            if !success || !saved {
                std::process::exit(1);
            }
        } else {
            app::run();
        }
    }
    #[cfg(not(windows))]
    eprintln!("HyperSearch GUI requires Windows");
}
