//! Generates the embedded word lists and trigram models.
//!
//! Usage: datagen <en_50k.txt> <ru_50k.txt> <out_dir> [max_words]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::{env, fs, process};
use swift_engine::keymap::{Layout, is_letter};

fn alphabet(layout: Layout) -> Vec<char> {
    match layout {
        Layout::En => ('a'..='z').collect(),
        Layout::Ru => ('а'..='я').chain(['ё']).collect(),
    }
}

/// Parses `word count` lines; returns lowercased letter-only words with counts, in file order.
fn parse(input: &str, layout: Layout, max_words: usize) -> Vec<(String, u64)> {
    let mut out: Vec<(String, u64)> = Vec::new();
    for line in input.lines() {
        let Some((word, count)) = line.split_once(' ') else {
            continue;
        };
        let Ok(count) = count.trim().parse::<u64>() else {
            continue;
        };
        let word = word.to_lowercase();
        if word.is_empty() || !word.chars().all(|c| is_letter(c, layout)) {
            continue;
        }
        out.push((word, count));
        if out.len() >= max_words {
            break;
        }
    }
    out
}

fn build(words: &[(String, u64)], layout: Layout) -> (String, String) {
    let mut list = String::new();
    let mut tri: HashMap<[char; 3], f64> = HashMap::new();
    let mut bi: HashMap<[char; 2], f64> = HashMap::new();
    for (w, freq) in words {
        list.push_str(w);
        list.push('\n');
        let weight = (*freq as f64).ln() + 1.0;
        let padded: Vec<char> = "^^".chars().chain(w.chars()).chain(['$']).collect();
        for t in padded.windows(3) {
            *tri.entry([t[0], t[1], t[2]]).or_default() += weight;
            *bi.entry([t[0], t[1]]).or_default() += weight;
        }
    }
    let v = (alphabet(layout).len() + 1) as f64;
    let mut rows: Vec<(String, f64)> = tri
        .iter()
        .map(|(t, c)| {
            let denom = bi[&[t[0], t[1]]] + 0.1 * v;
            (t.iter().collect(), ((c + 0.1) / denom).log10())
        })
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    let floor = rows.iter().map(|r| r.1).fold(f64::INFINITY, f64::min) - 1.0;
    let mut model = String::new();
    writeln!(model, "#floor\t{floor:.4}").unwrap();
    for (t, p) in rows {
        writeln!(model, "{t}\t{p:.4}").unwrap();
    }
    (list, model)
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: datagen <en_50k.txt> <ru_50k.txt> <out_dir> [max_words]");
        process::exit(2);
    }
    let max_words = args
        .get(4)
        .map_or(usize::MAX, |s| s.parse().expect("max_words"));
    for (path, layout, name) in [(&args[1], Layout::En, "en"), (&args[2], Layout::Ru, "ru")] {
        let input = fs::read_to_string(path).expect("read input");
        let words = parse(&input, layout, max_words);
        let (list, model) = build(&words, layout);
        fs::write(format!("{}/{name}.words", args[3]), list).expect("write words");
        fs::write(format!("{}/{name}.tri", args[3]), model).expect("write tri");
        eprintln!("{name}: {} words", words.len());
    }
}
