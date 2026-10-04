//! Which paired devices Hop should send.
//!
//! Written as JSON, to a temporary file first, so a crash cannot leave an empty
//! choice. A file that does not parse is an error. `HOP_CONFIG_DIR` overrides
//! the directory.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    addresses: Vec<String>,
}

pub fn choice_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("HOP_CONFIG_DIR") {
        if !dir.is_empty() {
            return PathBuf::from(dir).join("choice.json");
        }
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        return home
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Hop/choice.json");
    }
    #[cfg(not(target_os = "macos"))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        home.unwrap_or_else(|| PathBuf::from("."))
            .join(".config/hop/choice.json")
    }
}

pub fn load(path: &Path) -> io::Result<HashSet<String>> {
    match fs::read_to_string(path) {
        Ok(text) => parse_json(&text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => load_legacy(path),
        Err(err) => Err(err),
    }
}

pub fn save(path: &Path, chosen: &HashSet<String>) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut addresses: Vec<_> = chosen.iter().map(|address| normalize(address)).collect();
    addresses.sort();
    addresses.dedup();
    let text = serde_json::to_string_pretty(&File { addresses })
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text + "\n")?;
    fs::rename(&tmp, path)?;
    Ok(())
}

fn parse_json(text: &str) -> io::Result<HashSet<String>> {
    let file: File = serde_json::from_str(text)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    Ok(file
        .addresses
        .iter()
        .map(|address| normalize(address))
        .filter(|address| !address.is_empty())
        .collect())
}

/// `chosen.txt` from before the JSON file. Used only when `choice.json` is absent.
fn load_legacy(json_path: &Path) -> io::Result<HashSet<String>> {
    let legacy = json_path.with_file_name("chosen.txt");
    match fs::read_to_string(&legacy) {
        Ok(text) => Ok(text
            .lines()
            .map(normalize)
            .filter(|line| !line.is_empty())
            .collect()),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(err) => Err(err),
    }
}

fn normalize(line: &str) -> String {
    line.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{load, save};
    use std::collections::HashSet;
    use std::fs;

    fn dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "hop-choice-{name}-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn round_trip_normalizes_addresses() {
        let path = dir("round").join("choice.json");
        let mut chosen = HashSet::new();
        chosen.insert("AA-BB-CC-DD-EE-FF".into());
        chosen.insert("aa-bb-cc-dd-ee-ff".into());
        save(&path, &chosen).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        assert_eq!(
            load(&path).unwrap(),
            HashSet::from(["aa-bb-cc-dd-ee-ff".to_string()])
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_nothing_chosen() {
        let path = dir("missing").join("choice.json");
        assert!(load(&path).unwrap().is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn a_file_that_does_not_parse_is_an_error() {
        let path = dir("bad").join("choice.json");
        fs::write(&path, "not json\n").unwrap();
        assert!(load(&path).is_err());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn an_old_text_file_is_read_when_json_is_absent() {
        let folder = dir("legacy");
        fs::write(folder.join("chosen.txt"), "AA-BB\n\ncc-dd\n").unwrap();
        let loaded = load(&folder.join("choice.json")).unwrap();
        assert_eq!(
            loaded,
            HashSet::from(["aa-bb".to_string(), "cc-dd".to_string()])
        );
        let _ = fs::remove_dir_all(folder);
    }
}
