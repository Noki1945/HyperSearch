//! Stream the saved index and estimate the current engine's selectable profiles.
use hypersearch_engine::planning;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os().nth(1).ok_or("index path required")?;
    println!(
        "{}",
        planning::analyze_index(std::path::Path::new(&path))?.description()
    );
    println!(
        "No Volume or SearchIndex was built. This is an allocation estimate, not a measurement or a checksum verification."
    );
    Ok(())
}
