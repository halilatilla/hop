//! Which paired devices Hop should send. One Bluetooth address per line.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub fn chosen_path() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        return home
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Hop/chosen.txt");
    }
    #[cfg(not(target_os = "macos"))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        home.unwrap_or_else(|| PathBuf::from("."))
            .join(".config/hop/chosen.txt")
    }
}

pub fn load(path: &Path) -> HashSet<String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return HashSet::new(),
        Err(err) => {
            eprintln!("hop: could not read chosen devices ({err})");
            return HashSet::new();
        }
    };
    text.lines()
        .map(normalize)
        .filter(|line| !line.is_empty())
        .collect()
}

pub fn save(path: &Path, chosen: &HashSet<String>) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut lines: Vec<_> = chosen.iter().map(|address| normalize(address)).collect();
    lines.sort();
    lines.dedup();
    let mut body = lines.join("\n");
    if !body.is_empty() {
        body.push('\n');
    }
    std::fs::write(path, body)
}

fn normalize(line: &str) -> String {
    line.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{load, save};
    use std::collections::HashSet;

    #[test]
    fn round_trip_normalizes_addresses() {
        let path = std::env::temp_dir().join(format!("hop-chosen-{}.txt", std::process::id()));
        let mut chosen = HashSet::new();
        chosen.insert("AA-BB-CC-DD-EE-FF".into());
        save(&path, &chosen).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded, HashSet::from(["aa-bb-cc-dd-ee-ff".to_string()]));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn missing_file_is_nothing_chosen() {
        let path = std::env::temp_dir().join(format!(
            "hop-chosen-missing-{}-{}.txt",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_file(&path);
        assert!(load(&path).is_empty());
    }
}
