#![cfg_attr(windows, windows_subsystem = "windows")]
#[cfg(windows)]
mod app;
#[cfg(windows)]
mod settings;
#[cfg(windows)]
mod view_model;
fn main() {
    #[cfg(windows)]
    app::run();
    #[cfg(not(windows))]
    eprintln!("HyperSearch GUI requires Windows");
}
