//! The shared list of devices either Mac may take.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Choice {
    pub addresses: HashSet<String>,
    pub names: HashMap<String, String>,
    pub quiet: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    addresses: Vec<String>,
    #[serde(default)]
    names: HashMap<String, String>,
    #[serde(default)]
    quiet: Vec<String>,
}

pub fn config_dir() -> PathBuf {
    choice_path()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn choice_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("HOP_CONFIG_DIR")
        && !dir.is_empty()
    {
        return PathBuf::from(dir).join("choice.json");
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        home.unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/Hop/choice.json")
    }
    #[cfg(not(target_os = "macos"))]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        home.unwrap_or_else(|| PathBuf::from("."))
            .join(".config/hop/choice.json")
    }
}

pub fn load(path: &Path) -> io::Result<Choice> {
    match fs::read_to_string(path) {
        Ok(text) => parse_json(&text),
        Err(err) if err.kind() == io::ErrorKind::NotFound => load_legacy(path),
        Err(err) => Err(err),
    }
}

pub fn save(path: &Path, choice: &Choice) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }
    }
    let mut addresses: Vec<_> = choice
        .addresses
        .iter()
        .map(|address| normalize(address))
        .filter(|address| !address.is_empty())
        .collect();
    addresses.sort();
    addresses.dedup();
    let names = choice
        .names
        .iter()
        .filter_map(|(address, name)| {
            let address = normalize(address);
            let name = name.trim();
            if address.is_empty() || name.is_empty() {
                None
            } else {
                Some((address, name.to_string()))
            }
        })
        .collect();
    let mut quiet: Vec<_> = choice
        .quiet
        .iter()
        .map(|address| normalize(address))
        .filter(|address| !address.is_empty())
        .collect();
    quiet.sort();
    quiet.dedup();
    let text = serde_json::to_string_pretty(&File {
        addresses,
        names,
        quiet,
    })
    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        file.write_all((text + "\n").as_bytes())?;
    }
    #[cfg(not(unix))]
    {
        fs::write(&tmp, text + "\n")?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn absorb(choice: &mut Choice, add: &[(String, String)], remove: &[String]) {
    for address in remove {
        let address = normalize(address);
        if address.is_empty() {
            continue;
        }
        choice.addresses.remove(&address);
        choice.names.remove(&address);
        choice.quiet.remove(&address);
    }
    for (address, name) in add {
        let address = normalize(address);
        if address.is_empty() || choice.quiet.contains(&address) {
            continue;
        }
        choice.addresses.insert(address.clone());
        let name = name.trim();
        if !name.is_empty() {
            choice.names.insert(address, name.to_string());
        }
    }
}

fn parse_json(text: &str) -> io::Result<Choice> {
    let file: File = serde_json::from_str(text)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err.to_string()))?;
    let mut choice = Choice::default();
    let add: Vec<_> = file
        .addresses
        .iter()
        .map(|address| {
            (
                address.clone(),
                file.names.get(address).cloned().unwrap_or_default(),
            )
        })
        .collect();
    absorb(&mut choice, &add, &[]);
    for address in &file.quiet {
        let address = normalize(address);
        if address.is_empty() {
            continue;
        }
        choice.addresses.remove(&address);
        choice.quiet.insert(address);
    }
    Ok(choice)
}

fn load_legacy(json_path: &Path) -> io::Result<Choice> {
    let legacy = json_path.with_file_name("chosen.txt");
    match fs::read_to_string(&legacy) {
        Ok(text) => {
            let mut choice = Choice::default();
            let add: Vec<_> = text
                .lines()
                .map(|line| (line.to_string(), String::new()))
                .collect();
            absorb(&mut choice, &add, &[]);
            Ok(choice)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(Choice::default()),
        Err(err) => Err(err),
    }
}

fn normalize(line: &str) -> String {
    crate::wire::canon(line.trim())
}

#[cfg(test)]
mod tests {
    use super::{Choice, absorb, load, save};
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
        let mut choice = Choice::default();
        choice.addresses.insert("AA-BB-CC-DD-EE-FF".into());
        choice.addresses.insert("aa-bb-cc-dd-ee-ff".into());
        choice
            .names
            .insert("aa-bb-cc-dd-ee-ff".into(), "Mouse".into());
        save(&path, &choice).unwrap();
        assert!(!path.with_extension("json.tmp").exists());
        let loaded = load(&path).unwrap();
        assert_eq!(
            loaded.addresses,
            HashSet::from(["aa-bb-cc-dd-ee-ff".to_string()])
        );
        assert_eq!(
            loaded.names.get("aa-bb-cc-dd-ee-ff").map(String::as_str),
            Some("Mouse")
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn missing_file_is_nothing_chosen() {
        let path = dir("missing").join("choice.json");
        assert!(load(&path).unwrap().addresses.is_empty());
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
            loaded.addresses,
            HashSet::from(["aa-bb".to_string(), "cc-dd".to_string()])
        );
        let _ = fs::remove_dir_all(folder);
    }

    #[test]
    fn a_removal_waiting_to_be_told_is_not_added_back() {
        let mut choice = Choice::default();
        choice.quiet.insert("aa-bb".into());
        absorb(
            &mut choice,
            &[
                ("aa-bb".into(), "Mouse".into()),
                ("cc-dd".into(), "Keys".into()),
            ],
            &["ee-ff".into()],
        );
        assert!(!choice.addresses.contains("aa-bb"));
        assert!(choice.addresses.contains("cc-dd"));
        assert_eq!(choice.names.get("cc-dd").map(String::as_str), Some("Keys"));
    }
}
