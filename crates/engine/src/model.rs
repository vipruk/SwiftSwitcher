//! Embedded per-language word sets and character trigram models.

use crate::keymap::Layout;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// A language model: a known-word set plus a smoothed character trigram table.
pub struct LangModel {
    words: HashSet<&'static str>,
    trigrams: HashMap<[char; 3], f32>,
    floor: f32,
}

impl LangModel {
    fn parse(words: &'static str, tri: &'static str) -> LangModel {
        let mut floor = -10.0;
        let mut trigrams = HashMap::new();
        for line in tri.lines() {
            let Some((key, val)) = line.split_once('\t') else {
                continue;
            };
            let Ok(val) = val.parse::<f32>() else {
                continue;
            };
            if key == "#floor" {
                floor = val;
                continue;
            }
            let chars: Vec<char> = key.chars().collect();
            if let [a, b, c] = chars[..] {
                trigrams.insert([a, b, c], val);
            }
        }
        LangModel {
            words: words.lines().collect(),
            trigrams,
            floor,
        }
    }

    pub fn embedded(layout: Layout) -> &'static LangModel {
        static EN: OnceLock<LangModel> = OnceLock::new();
        static RU: OnceLock<LangModel> = OnceLock::new();
        match layout {
            Layout::En => EN.get_or_init(|| {
                LangModel::parse(
                    include_str!("../data/en.words"),
                    include_str!("../data/en.tri"),
                )
            }),
            Layout::Ru => RU.get_or_init(|| {
                LangModel::parse(
                    include_str!("../data/ru.words"),
                    include_str!("../data/ru.tri"),
                )
            }),
        }
    }

    pub fn contains(&self, lower_word: &str) -> bool {
        self.words.contains(lower_word)
    }

    /// Mean trigram log10 probability over `^^word$`; higher is more plausible.
    pub fn score(&self, lower_word: &str) -> f32 {
        if lower_word.is_empty() {
            return self.floor;
        }
        let padded: Vec<char> = "^^"
            .chars()
            .chain(lower_word.chars())
            .chain(['$'])
            .collect();
        let sum: f32 = padded
            .windows(3)
            .map(|t| {
                self.trigrams
                    .get(&[t[0], t[1], t[2]])
                    .copied()
                    .unwrap_or(self.floor)
            })
            .sum();
        sum / (padded.len() - 2) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contains_known_words() {
        assert!(LangModel::embedded(Layout::Ru).contains("привет"));
        assert!(LangModel::embedded(Layout::En).contains("hello"));
        assert!(!LangModel::embedded(Layout::En).contains("ghbdtn"));
    }

    #[test]
    fn score_prefers_real_words() {
        let en = LangModel::embedded(Layout::En);
        assert!(en.score("hello") > en.score("ghbdtn") + 1.0);
        let ru = LangModel::embedded(Layout::Ru);
        assert!(ru.score("привет") > ru.score("руддщ") + 1.0);
    }
}
