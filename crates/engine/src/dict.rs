//! User dictionaries: words the user never wants converted, or always wants converted.
//! Words are stored lowercased, as the typed reading (text in the layout it was typed in).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

#[derive(Default, Clone, Debug)]
pub struct UserDict {
    pub never: HashSet<String>,
    pub always: HashSet<String>,
    /// Undo counts of words not yet in the never list, kept in `undos.txt`.
    pub undos: HashMap<String, u32>,
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

/// Reads `word count` lines; malformed lines are skipped.
fn read_counts(path: &Path) -> io::Result<HashMap<String, u32>> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(s
            .lines()
            .filter_map(|l| {
                let (w, n) = l.trim().rsplit_once(' ')?;
                Some((w.trim().to_lowercase(), n.parse().ok()?))
            })
            .filter(|(w, _)| !w.is_empty())
            .collect()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(HashMap::new()),
        Err(e) => Err(e),
    }
}

impl UserDict {
    /// Loads `never.txt` and `always.txt` from `dir`. Missing files or directory give empty sets.
    pub fn load(dir: &Path) -> io::Result<UserDict> {
        Ok(UserDict {
            never: read_set(&dir.join("never.txt"))?,
            always: read_set(&dir.join("always.txt"))?,
            undos: read_counts(&dir.join("undos.txt"))?,
        })
    }

    /// Records one undo of `word`. Once it has been undone `threshold` times
    /// it moves to the never list. Returns true when the word was learned.
    pub fn record_undo(&mut self, dir: &Path, word: &str, threshold: u32) -> io::Result<bool> {
        let word = word.trim().to_lowercase();
        if word.is_empty() || word.contains(char::is_whitespace) || self.never.contains(&word) {
            return Ok(false);
        }
        let n = self.undos.entry(word.clone()).or_insert(0);
        *n += 1;
        let learned = *n >= threshold.max(1);
        if learned {
            self.undos.remove(&word);
        }
        self.write_undos(dir)?;
        if learned {
            self.add_never(dir, &word)?;
        }
        Ok(learned)
    }

    fn write_undos(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)?;
        let mut words: Vec<_> = self.undos.iter().collect();
        words.sort();
        let text: String = words.iter().map(|(w, n)| format!("{w} {n}\n")).collect();
        let tmp = dir.join("undos.txt.tmp");
        fs::write(&tmp, text)?;
        fs::rename(tmp, dir.join("undos.txt"))
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

    #[test]
    fn record_undo_learns_at_threshold_and_persists_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sub");
        let mut d = UserDict::default();
        assert!(!d.record_undo(&dir, "Ghbdtn", 3).unwrap());
        assert!(!d.record_undo(&dir, "ghbdtn", 3).unwrap());
        assert!(!d.never.contains("ghbdtn"));
        // Counts survive a restart.
        let mut d = UserDict::load(&dir).unwrap();
        assert_eq!(d.undos.get("ghbdtn"), Some(&2));
        assert!(d.record_undo(&dir, "ghbdtn", 3).unwrap());
        let loaded = UserDict::load(&dir).unwrap();
        assert!(loaded.never.contains("ghbdtn"));
        assert!(loaded.undos.is_empty());
        // Already learned: nothing more is counted.
        assert!(!d.record_undo(&dir, "ghbdtn", 3).unwrap());
        assert!(UserDict::load(&dir).unwrap().undos.is_empty());
    }

    #[test]
    fn record_undo_threshold_zero_or_one_learns_at_once() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = UserDict::default();
        assert!(d.record_undo(tmp.path(), "rjl", 0).unwrap());
        assert!(d.record_undo(tmp.path(), "ckjdj", 1).unwrap());
        assert_eq!(UserDict::load(tmp.path()).unwrap().never.len(), 2);
    }
}
