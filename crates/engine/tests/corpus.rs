//! Corpus acceptance tests: precision (no false corrections) and recall of `decide`.
//!
//! Precision: every token typed in its own layout must be kept.
//! Recall: tokens typed in the wrong layout (>= 5 letters) should be converted.
//! The full-corpus test needs `scripts/fetch-corpus.sh` and is ignored by default.

use std::fs;
use std::path::PathBuf;

use swift_engine::detect::{Params, Verdict, decide};
use swift_engine::dict::UserDict;
use swift_engine::keymap::{Layout, Stroke, is_letter, stroke_for};

const RECALL_FLOOR: f64 = 0.80;
const RECALL_MIN_LETTERS: usize = 5;

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

/// Strokes that type `tok` in `layout`, or `None` if some char has no key there.
fn typed(tok: &str, layout: Layout) -> Option<Vec<Stroke>> {
    tok.chars().map(|c| stroke_for(c, layout)).collect()
}

fn converts(strokes: &[Stroke], typed_in: Layout) -> bool {
    decide(strokes, typed_in, &UserDict::default(), &Params::default()) == Verdict::Convert
}

#[derive(Default)]
struct Stats {
    tokens: usize,
    skipped: usize,
    false_corrections: Vec<String>,
    recall_total: usize,
    recall_hits: usize,
    missed: Vec<String>,
}

impl Stats {
    fn recall(&self) -> f64 {
        if self.recall_total == 0 {
            return 1.0;
        }
        self.recall_hits as f64 / self.recall_total as f64
    }

    fn print(&self, name: &str) {
        println!(
            "{name}: tokens={} skipped={} false_corrections={} recall={}/{}={:.3}",
            self.tokens,
            self.skipped,
            self.false_corrections.len(),
            self.recall_hits,
            self.recall_total,
            self.recall(),
        );
    }
}

/// Runs precision and recall over whitespace-separated tokens of `text`, all in `lang`.
fn evaluate<'a>(tokens: impl Iterator<Item = &'a str>, lang: Layout) -> Stats {
    let mut st = Stats::default();
    for tok in tokens {
        let Some(strokes) = typed(tok, lang) else {
            st.skipped += 1;
            continue;
        };
        st.tokens += 1;
        if converts(&strokes, lang) {
            st.false_corrections.push(tok.to_string());
        }
        let letters = tok.chars().filter(|&c| is_letter(c, lang)).count();
        if letters >= RECALL_MIN_LETTERS {
            st.recall_total += 1;
            if converts(&strokes, lang.other()) {
                st.recall_hits += 1;
            } else {
                st.missed.push(tok.to_string());
            }
        }
    }
    st
}

fn sample(file: &str, lang: Layout) -> Stats {
    let text = fs::read_to_string(corpus_dir().join(file)).unwrap();
    let st = evaluate(text.split_whitespace(), lang);
    st.print(file);
    st
}

#[test]
fn no_false_corrections_sample_en() {
    let st = sample("sample-en.txt", Layout::En);
    assert!(
        st.false_corrections.is_empty(),
        "false corrections: {:?}",
        st.false_corrections
    );
}

#[test]
fn no_false_corrections_sample_ru() {
    let st = sample("sample-ru.txt", Layout::Ru);
    assert!(
        st.false_corrections.is_empty(),
        "false corrections: {:?}",
        st.false_corrections
    );
}

/// The tech corpus is typed in `En`; the few Cyrillic tokens (RU abbreviations) are typed in `Ru`.
#[test]
fn no_false_corrections_tech() {
    let text = fs::read_to_string(corpus_dir().join("tech.txt")).unwrap();
    let mut tokens = 0;
    let mut skipped = 0;
    let mut offenders = Vec::new();
    for tok in text.split_whitespace() {
        let Some((strokes, lang)) = [Layout::En, Layout::Ru]
            .into_iter()
            .find_map(|l| typed(tok, l).map(|s| (s, l)))
        else {
            skipped += 1;
            continue;
        };
        tokens += 1;
        if converts(&strokes, lang) {
            offenders.push(tok);
        }
    }
    println!(
        "tech.txt: tokens={tokens} skipped={skipped} false_corrections={}",
        offenders.len()
    );
    assert!(tokens >= 300, "tech corpus too small: {tokens}");
    assert!(offenders.is_empty(), "false corrections: {offenders:?}");
}

#[test]
fn recall_report() {
    for (file, lang) in [("sample-en.txt", Layout::En), ("sample-ru.txt", Layout::Ru)] {
        let st = sample(file, lang);
        println!("{file}: missed {:?}", st.missed);
        assert!(
            st.recall() >= RECALL_FLOOR,
            "{file}: recall {:.3} < {RECALL_FLOOR}",
            st.recall()
        );
    }
}

#[test]
#[ignore = "needs scripts/fetch-corpus.sh"]
fn full_corpus_precision() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/corpus");
    for (file, lang) in [
        ("eng-sentences.txt", Layout::En),
        ("rus-sentences.txt", Layout::Ru),
    ] {
        let path = dir.join(file);
        let text = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{}: {e}; run scripts/fetch-corpus.sh", path.display()));
        let sentences = text
            .lines()
            .map(|l| l.split_once('\t').map_or(l, |(_, s)| s));
        let st = evaluate(sentences.flat_map(str::split_whitespace), lang);
        st.print(file);
        println!("{file}: false corrections {:?}", st.false_corrections);
        assert!(
            st.false_corrections.len() * 10_000 < st.tokens,
            "{file}: {} false corrections in {} tokens",
            st.false_corrections.len(),
            st.tokens
        );
    }
}
