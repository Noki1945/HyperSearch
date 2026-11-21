use hypersearch_engine::search::MemoryMode;
use std::{io, path::Path};
#[derive(Clone, Copy)]
pub struct Preference {
    pub mode: MemoryMode,
    pub budget_mb: Option<usize>,
}
impl Preference {
    pub fn load(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let mut lines = text.lines();
        let mode = MemoryMode::parse(lines.next()?)?;
        let budget_mb = lines.next().and_then(|s| s.parse().ok());
        Some(Self { mode, budget_mb })
    }
    pub fn save(self, path: &Path) -> io::Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(
            path,
            format!(
                "{}\n{}\n",
                self.mode.key(),
                self.budget_mb.map(|x| x.to_string()).unwrap_or_default()
            ),
        )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_preferences_and_invalid_mode() {
        let path =
            std::env::temp_dir().join(format!("hypersearch-settings-{}.txt", std::process::id()));
        let p = Preference {
            mode: MemoryMode::Economy,
            budget_mb: Some(384),
        };
        p.save(&path).unwrap();
        let loaded = Preference::load(&path).unwrap();
        assert_eq!(loaded.mode, p.mode);
        assert_eq!(loaded.budget_mb, p.budget_mb);
        std::fs::write(&path, "unknown\n100").unwrap();
        assert!(Preference::load(&path).is_none());
        std::fs::remove_file(path).unwrap();
    }
}
