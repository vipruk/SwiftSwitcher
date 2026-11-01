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
    /// Manual-conversion counts of words not yet in the always list, kept in
    /// `converts.txt`.
    pub converts: HashMap<String, u32>,
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

/// Writes `word count` lines, sorted, atomically.
fn write_counts(dir: &Path, name: &str, counts: &HashMap<String, u32>) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let mut words: Vec<_> = counts.iter().collect();
    words.sort();
    let text: String = words.iter().map(|(w, n)| format!("{w} {n}\n")).collect();
    let tmp = dir.join(format!("{name}.tmp"));
    fs::write(&tmp, text)?;
    fs::rename(tmp, dir.join(name))
}

/// Which user list an edit applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    Never,
    Always,
}

impl UserDict {
    /// Loads `never.txt` and `always.txt` from `dir`. Missing files or directory give empty sets.
    pub fn load(dir: &Path) -> io::Result<UserDict> {
        Ok(UserDict {
            never: read_set(&dir.join("never.txt"))?,
            always: read_set(&dir.join("always.txt"))?,
            undos: read_counts(&dir.join("undos.txt"))?,
            converts: read_counts(&dir.join("converts.txt"))?,
        })
    }

    /// Records one undo of `word`. Once it has been undone `threshold` times
    /// it moves to the never list (and leaves the always list). Returns true
    /// when the word was learned.
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
        write_counts(dir, "undos.txt", &self.undos)?;
        if learned {
            self.forget_always(dir, &word)?;
            self.add_never(dir, &word)?;
        }
        Ok(learned)
    }

    /// Records one manual conversion of `word` (the typed reading). Once it
    /// has been converted by hand `threshold` times it moves to the always
    /// list (and leaves the never list). A threshold of 0 turns this off.
    /// Returns true when the word was learned.
    pub fn record_convert(&mut self, dir: &Path, word: &str, threshold: u32) -> io::Result<bool> {
        let word = word.trim().to_lowercase();
        if threshold == 0
            || word.is_empty()
            || word.contains(char::is_whitespace)
            || self.always.contains(&word)
        {
            return Ok(false);
        }
        let n = self.converts.entry(word.clone()).or_insert(0);
        *n += 1;
        let learned = *n >= threshold;
        if learned {
            self.converts.remove(&word);
        }
        write_counts(dir, "converts.txt", &self.converts)?;
        if learned {
            if self.never.remove(&word) {
                Self::save_edit(dir, List::Never, &[], std::slice::from_ref(&word))?;
            }
            if self.undos.remove(&word).is_some() {
                write_counts(dir, "undos.txt", &self.undos)?;
            }
            Self::save_edit(dir, List::Always, std::slice::from_ref(&word), &[])?;
            self.always.insert(word);
        }
        Ok(learned)
    }

    /// Drops `word` from the always list and its manual-conversion count.
    fn forget_always(&mut self, dir: &Path, word: &str) -> io::Result<()> {
        if self.always.remove(word) {
            Self::save_edit(dir, List::Always, &[], &[word.to_string()])?;
        }
        if self.converts.remove(word).is_some() {
            write_counts(dir, "converts.txt", &self.converts)?;
        }
        Ok(())
    }

    /// Edits one list file. The file on disk is the source of truth: it is
    /// re-read right before writing, so words the daemon appended meanwhile are
    /// kept. The result is sorted and written atomically.
    pub fn save_edit(dir: &Path, list: List, add: &[String], remove: &[String]) -> io::Result<()> {
        let name = match list {
            List::Never => "never.txt",
            List::Always => "always.txt",
        };
        let path = dir.join(name);
        let mut words = read_set(&path)?;
        for w in remove {
            words.remove(&w.trim().to_lowercase());
        }
        for w in add {
            let w = w.trim().to_lowercase();
            if !w.is_empty() && !w.contains('\n') {
                words.insert(w);
            }
        }
        let mut sorted: Vec<_> = words.into_iter().collect();
        sorted.sort();
        let text: String = sorted.iter().map(|w| format!("{w}\n")).collect();
        fs::create_dir_all(dir)?;
        let tmp = dir.join(format!("{name}.tmp"));
        fs::write(&tmp, text)?;
        fs::rename(tmp, path)
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

    #[test]
    fn record_convert_learns_always_at_threshold_and_persists_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sub");
        let mut d = UserDict::default();
        assert!(!d.record_convert(&dir, "Секд", 2).unwrap());
        assert!(!d.always.contains("секд"));
        // Counts survive a restart.
        let mut d = UserDict::load(&dir).unwrap();
        assert_eq!(d.converts.get("секд"), Some(&1));
        assert!(d.record_convert(&dir, "секд", 2).unwrap());
        let loaded = UserDict::load(&dir).unwrap();
        assert!(loaded.always.contains("секд"));
        assert!(loaded.converts.is_empty());
        // Already learned: nothing more is counted.
        assert!(!d.record_convert(&dir, "секд", 2).unwrap());
        assert!(UserDict::load(&dir).unwrap().converts.is_empty());
    }

    #[test]
    fn record_convert_threshold_zero_is_off() {
        let tmp = tempfile::tempdir().unwrap();
        let mut d = UserDict::default();
        assert!(!d.record_convert(tmp.path(), "секд", 0).unwrap());
        assert!(!d.record_convert(tmp.path(), "секд", 0).unwrap());
        let loaded = UserDict::load(tmp.path()).unwrap();
        assert!(loaded.always.is_empty() && loaded.converts.is_empty());
    }

    #[test]
    fn learning_one_list_drops_the_word_from_the_other() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut d = UserDict::default();
        assert!(d.record_undo(dir, "ghbdtn", 1).unwrap());
        assert!(d.record_convert(dir, "ghbdtn", 1).unwrap());
        let loaded = UserDict::load(dir).unwrap();
        assert!(loaded.always.contains("ghbdtn"));
        assert!(!loaded.never.contains("ghbdtn"));
        assert!(!d.never.contains("ghbdtn"));

        assert!(d.record_undo(dir, "ghbdtn", 1).unwrap());
        let loaded = UserDict::load(dir).unwrap();
        assert!(loaded.never.contains("ghbdtn"));
        assert!(!loaded.always.contains("ghbdtn"));
        assert!(!d.always.contains("ghbdtn"));
    }

    fn lines(p: &Path) -> String {
        fs::read_to_string(p).unwrap()
    }

    #[test]
    fn save_edit_add_remove() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("never.txt"), "a\nb\n").unwrap();
        UserDict::save_edit(tmp.path(), List::Never, &["c".into()], &["a".into()]).unwrap();
        assert_eq!(lines(&tmp.path().join("never.txt")), "b\nc\n");
        assert!(!tmp.path().join("never.txt.tmp").exists());
    }

    #[test]
    fn save_edit_always_creates_dir_and_sorts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sub");
        UserDict::save_edit(&dir, List::Always, &["Zeta".into(), "alpha".into()], &[]).unwrap();
        assert_eq!(lines(&dir.join("always.txt")), "alpha\nzeta\n");
    }

    #[test]
    fn save_merges_concurrent_additions() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("never.txt");
        fs::write(&path, "a\n").unwrap();
        let _loaded = UserDict::load(tmp.path()).unwrap();
        let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(f, "x").unwrap();
        UserDict::save_edit(tmp.path(), List::Never, &["y".into()], &[]).unwrap();
        assert_eq!(lines(&path), "a\nx\ny\n");
    }
}
