//! User dictionaries: words the user never wants converted, or always wants converted.
//! Words are stored lowercased, as the typed reading (text in the layout it was typed in).

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

#[derive(Default, Clone, Debug)]
pub struct UserDict {
    pub never: HashSet<String>,
    pub always: HashSet<String>,
}

fn read_set(path: &Path) -> io::Result<HashSet<String>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s
            .lines()
            .map(|l| l.trim().to_lowercase())
            .filter(|l| !l.is_empty())
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(HashSet::new()),
        Err(e) => Err(e),
    }
}

impl UserDict {
    /// Loads `never.txt` and `always.txt` from `dir`. Missing files or directory give empty sets.
    pub fn load(dir: &Path) -> io::Result<UserDict> {
        Ok(UserDict {
            never: read_set(&dir.join("never.txt"))?,
            always: read_set(&dir.join("always.txt"))?,
        })
    }

    /// Adds a word to the never list, in memory and appended to `never.txt`.
    pub fn add_never(&mut self, dir: &Path, word: &str) -> io::Result<()> {
        let word = word.trim().to_lowercase();
        if word.is_empty() || word.contains('\n') || !self.never.insert(word.clone()) {
            return Ok(());
        }
        fs::create_dir_all(dir)?;
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("never.txt"))?;
        writeln!(f, "{word}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_dir_is_empty() {
        let d = UserDict::load(Path::new("/nonexistent/swiftswitcher-test")).unwrap();
        assert!(d.never.is_empty() && d.always.is_empty());
    }

    #[test]
    fn add_never_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sub");
        let mut d = UserDict::default();
        d.add_never(&dir, "Ghbdtn").unwrap();
        d.add_never(&dir, "ghbdtn").unwrap();
        d.add_never(&dir, "rjl").unwrap();
        let loaded = UserDict::load(&dir).unwrap();
        assert_eq!(loaded.never.len(), 2);
        assert!(loaded.never.contains("ghbdtn") && loaded.never.contains("rjl"));
        assert!(loaded.always.is_empty());
    }
}
