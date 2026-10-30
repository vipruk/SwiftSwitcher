//! Verdict rules: decide whether a typed word is a wrong-layout word that should be converted.
//! Precision beats recall: anything doubtful is kept as typed. Nothing here logs.

use crate::dict::UserDict;
use crate::keymap::{Layout, Stroke, char_for, is_letter};
use crate::model::LangModel;

/// Tuned on the corpus acceptance tests (`tests/corpus.rs`).
pub struct Params {
    /// Minimum letters in the other reading for any conversion.
    pub min_len: usize,
    /// Score lead the other reading needs when it is not a known word.
    pub margin: f32,
    /// Minimum score of the other reading when it is not a known word.
    pub floor: f32,
    /// Score lead the other reading needs even when it is a known word, so that a
    /// plausible typed reading (a name, an inflected form missing from the word list,
    /// an abbreviation) is kept.
    pub dict_margin: f32,
    /// Minimum letters in the other reading to convert on scores alone; shorter
    /// readings convert only when they are known words.
    pub score_min_len: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            min_len: 3,
            margin: 2.2,
            floor: -3.5,
            dict_margin: 1.0,
            score_min_len: 4,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reason {
    TooShort,
    Protected,
    InUserNever,
    TypedIsWord,
    OtherNotPlausible,
    NotLetters,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Keep(Reason),
    Convert,
}

/// Strips leading and trailing characters that are not letters of `layout`.
fn core(s: &str, layout: Layout) -> &str {
    s.trim_matches(|c| !is_letter(c, layout))
}

fn is_protected(text: &str) -> bool {
    if text.chars().any(|c| c.is_ascii_digit())
        || ["://", "@", "_", "\\", "$", "="]
            .iter()
            .any(|p| text.contains(p))
        || text.starts_with(['-', '/', '~'])
        || text.contains('~')
    {
        return true;
    }
    // A single trailing '/' after letters is a period in the other layout.
    let body = match text.strip_suffix('/') {
        Some(b) if b.chars().last().is_some_and(char::is_alphabetic) => b,
        _ => text,
    };
    if body.contains('/') {
        return true;
    }
    let chars: Vec<char> = text.chars().collect();
    if chars
        .windows(2)
        .any(|w| w[0].is_lowercase() && w[1].is_uppercase())
    {
        return true;
    }
    let letters: Vec<char> = chars
        .iter()
        .copied()
        .filter(|c| c.is_alphabetic())
        .collect();
    // One letter repeated (ыыы, zzz) is an interjection or filler, not a word.
    if letters.len() >= 2
        && letters
            .iter()
            .all(|c| c.to_lowercase().eq(letters[0].to_lowercase()))
    {
        return true;
    }
    let upper = letters.iter().filter(|c| c.is_uppercase()).count();
    // Acronyms of any length (РСФСР, NASDAQ) and short tokens with two or more
    // capitals (units and plurals: ГГц, IDs) are kept.
    (!letters.is_empty() && upper == letters.len()) || (letters.len() <= 4 && upper >= 2)
}

/// Physical key rows of the main block, as ranges of consecutive key codes.
const KEY_ROWS: [std::ops::RangeInclusive<u16>; 4] = [2..=13, 16..=27, 30..=40, 44..=53];

/// Shortest run of neighbouring keys on one row that counts as a keyboard walk.
const WALK_LEN: usize = 5;

/// True if the strokes contain a run of `WALK_LEN` neighbouring keys of one row
/// in either direction (zxcvbnm, qwert, йцукен): a password or filler, read
/// as plausible text in neither layout.
fn is_keyboard_walk(strokes: &[Stroke]) -> bool {
    let row = |code: u16| KEY_ROWS.iter().position(|r| r.contains(&code));
    let mut run = 1;
    let mut dir = 0;
    for w in strokes.windows(2) {
        let step = i32::from(w[1].code) - i32::from(w[0].code);
        let neighbours =
            step.abs() == 1 && row(w[0].code).is_some_and(|r| row(w[1].code) == Some(r));
        run = match (neighbours, step == dir) {
            (false, _) => 1,
            (true, true) => run + 1,
            (true, false) => 2,
        };
        dir = step;
        if run >= WALK_LEN {
            return true;
        }
    }
    false
}

const CONTRACTION_TAILS: [&str; 7] = ["s", "t", "d", "ll", "re", "ve", "m"];

fn is_en_contraction(core_typed: &str, en: &LangModel) -> bool {
    let lower = core_typed.to_lowercase();
    let mut parts = lower.split('\'');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(a), Some(b), None) if !a.is_empty() && !b.is_empty() => {
            (en.contains(a) && en.contains(b)) || CONTRACTION_TAILS.contains(&b)
        }
        _ => false,
    }
}

pub fn decide(strokes: &[Stroke], typed_in: Layout, dict: &UserDict, p: &Params) -> Verdict {
    let typed: Option<String> = strokes.iter().map(|&s| char_for(s, typed_in)).collect();
    let other: Option<String> = strokes
        .iter()
        .map(|&s| char_for(s, typed_in.other()))
        .collect();
    let (Some(typed), Some(other)) = (typed, other) else {
        return Verdict::Keep(Reason::NotLetters);
    };
    let typed_lower = typed.to_lowercase();

    let typed_core_lower = core(&typed, typed_in).to_lowercase();
    let listed = |set: &std::collections::HashSet<String>| {
        set.contains(&typed_lower) || set.contains(&typed_core_lower)
    };
    if listed(&dict.always) {
        return Verdict::Convert;
    }
    if listed(&dict.never) {
        return Verdict::Keep(Reason::InUserNever);
    }
    if is_protected(&typed) || is_keyboard_walk(strokes) {
        return Verdict::Keep(Reason::Protected);
    }

    let other_core = core(&other, typed_in.other());
    if other_core.is_empty() || other_core.chars().any(|c| !is_letter(c, typed_in.other())) {
        return Verdict::Keep(Reason::NotLetters);
    }
    if other_core.chars().count() < p.min_len {
        return Verdict::Keep(Reason::TooShort);
    }

    let typed_model = LangModel::embedded(typed_in);
    let other_model = LangModel::embedded(typed_in.other());
    let typed_core = core(&typed, typed_in).to_lowercase();
    if typed_model.contains(&typed_core)
        || (typed_in == Layout::En && is_en_contraction(&typed_core, typed_model))
    {
        return Verdict::Keep(Reason::TypedIsWord);
    }

    let other_lower = other_core.to_lowercase();
    let score_other = other_model.score(&other_lower);
    let score_typed = typed_model.score(&typed_core);
    if other_model.contains(&other_lower) && score_other - score_typed >= p.dict_margin {
        return Verdict::Convert;
    }
    if other_lower.chars().count() >= p.score_min_len
        && score_other >= p.floor
        && score_other - score_typed >= p.margin
    {
        Verdict::Convert
    } else {
        Verdict::Keep(Reason::OtherNotPlausible)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::stroke_for;

    fn typed(text: &str, layout: Layout) -> Vec<Stroke> {
        text.chars()
            .map(|c| stroke_for(c, layout).unwrap_or_else(|| panic!("unmappable char {c:?}")))
            .collect()
    }

    fn run(text: &str, l: Layout) -> Verdict {
        decide(&typed(text, l), l, &UserDict::default(), &Params::default())
    }

    #[test]
    fn converts_wrong_layout() {
        assert_eq!(run("ghbdtn", Layout::En), Verdict::Convert);
        assert_eq!(run("ghbdtn?", Layout::En), Verdict::Convert);
        assert_eq!(run("ghbdtn/", Layout::En), Verdict::Convert);
        assert_eq!(run("руддщ", Layout::Ru), Verdict::Convert);
    }

    #[test]
    fn keeps_real_words() {
        assert_eq!(run("hello", Layout::En), Verdict::Keep(Reason::TypedIsWord));
        assert_eq!(
            run("привет", Layout::Ru),
            Verdict::Keep(Reason::TypedIsWord)
        );
    }

    #[test]
    fn keeps_contractions() {
        assert_eq!(run("don't", Layout::En), Verdict::Keep(Reason::TypedIsWord));
        assert_eq!(run("it's", Layout::En), Verdict::Keep(Reason::TypedIsWord));
    }

    #[test]
    fn protected_tokens() {
        for w in [
            "git2",
            "--force",
            "https://x.ru",
            "my_var",
            "getUser",
            "NFR",
            "~/src",
            "/usr",
            "a/b",
        ] {
            assert_eq!(run(w, Layout::En), Verdict::Keep(Reason::Protected), "{w}");
        }
    }

    #[test]
    fn protects_acronyms_and_units() {
        // Pre-tuning false corrections from the Leipzig RU corpus: РСФСР -> HCACH, ГГц -> UUw.
        for w in ["РСФСР", "РСФСР,", "ГГц", "ГГц).", "NASDAQ", "IDs"] {
            let l = if w.is_ascii() { Layout::En } else { Layout::Ru };
            assert_eq!(run(w, l), Verdict::Keep(Reason::Protected), "{w}");
        }
        // Sentence-initial capitals are still converted.
        assert_eq!(run("Ghbdtn", Layout::En), Verdict::Convert);
    }

    #[test]
    fn protects_repeated_letter() {
        // ыыы -> sss was a false correction (sss is in the EN word list).
        for (w, l) in [
            ("ыыы", Layout::Ru),
            ("Ыыы!", Layout::Ru),
            ("zzz", Layout::En),
        ] {
            assert_eq!(run(w, l), Verdict::Keep(Reason::Protected), "{w}");
        }
    }

    #[test]
    fn protects_keyboard_walks() {
        // zxcvbnm -> ячсмить was a false correction; walks are typical passwords.
        for (w, l) in [
            ("zxcvbnm", Layout::En),
            ("qwert", Layout::En),
            ("trewq", Layout::En),
            ("asdfg", Layout::En),
            ("ячсмить", Layout::Ru),
        ] {
            assert_eq!(run(w, l), Verdict::Keep(Reason::Protected), "{w}");
        }
        // Four neighbours are not a walk; a direction change restarts the run.
        assert!(!is_keyboard_walk(&typed("asdf", Layout::En)));
        assert!(!is_keyboard_walk(&typed("asdsa", Layout::En)));
        assert_eq!(run("ghbdtn", Layout::En), Verdict::Convert);
    }

    #[test]
    fn known_word_needs_dict_margin() {
        // Plausible RU/EN typed readings whose other reading is a known word were
        // converted before tuning: мск -> vcr, Уфе -> Eat, кум -> rev, Nth -> Тер.
        for (w, l) in [
            ("мск", Layout::Ru),
            ("Уфе", Layout::Ru),
            ("кум", Layout::Ru),
            ("утв.", Layout::Ru),
            ("Nth", Layout::En),
        ] {
            assert_eq!(run(w, l), Verdict::Keep(Reason::OtherNotPlausible), "{w}");
        }
        assert_eq!(run("rfr", Layout::En), Verdict::Convert);
        assert_eq!(run("xnj", Layout::En), Verdict::Convert);
    }

    #[test]
    fn short_non_words_are_not_scored() {
        // dnf -> вта was converted on trigram scores alone.
        for w in ["dnf", "jwt"] {
            assert_eq!(
                run(w, Layout::En),
                Verdict::Keep(Reason::OtherNotPlausible),
                "{w}"
            );
        }
    }

    #[test]
    fn too_short() {
        assert_eq!(run("yt", Layout::En), Verdict::Keep(Reason::TooShort));
    }

    #[test]
    fn user_dict() {
        let mut d = UserDict::default();
        d.never.insert("ghbdtn".into());
        d.always.insert("lfnf".into());
        let p = Params::default();
        let s = typed("ghbdtn", Layout::En);
        assert_eq!(
            decide(&s, Layout::En, &d, &p),
            Verdict::Keep(Reason::InUserNever)
        );
        let s = typed("lfnf", Layout::En);
        assert_eq!(decide(&s, Layout::En, &d, &p), Verdict::Convert);
    }

    #[test]
    fn user_dict_ignores_surrounding_punctuation() {
        let p = Params::default();
        let mut d = UserDict::default();
        d.never.insert("ghbdtn".into());
        d.always.insert("lfnf".into());
        let s = typed("ghbdtn?", Layout::En);
        assert_eq!(
            decide(&s, Layout::En, &d, &p),
            Verdict::Keep(Reason::InUserNever)
        );
        let s = typed("lfnf,", Layout::En);
        assert_eq!(decide(&s, Layout::En, &d, &p), Verdict::Convert);
    }
}
