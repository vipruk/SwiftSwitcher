//! The GNOME Shell extension carries its own copy of the US/RU table in
//! `keymap.js`. This test keeps it in sync with the engine's keymap.

use swift_engine::keymap::{Layout, convert_text};

const KEYMAP_JS: &str = "../../extension/swiftswitcher@vipruk.github.io/keymap.js";

/// Extracts the double-quoted string literal assigned by `export const <name> = "...";`.
fn js_string(src: &str, name: &str) -> String {
    let marker = format!("export const {name} = \"");
    let start = src
        .find(&marker)
        .unwrap_or_else(|| panic!("`{marker}` not found in keymap.js"))
        + marker.len();
    let mut out = String::new();
    let mut chars = src[start..].chars();
    loop {
        match chars.next().expect("unterminated string literal") {
            '"' => return out,
            '\\' => match chars.next().expect("dangling escape") {
                c @ ('\\' | '"' | '\'') => out.push(c),
                c => panic!("unsupported escape \\{c}"),
            },
            c => out.push(c),
        }
    }
}

#[test]
fn js_tables_match_engine() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/");
    let src = std::fs::read_to_string(format!("{path}{KEYMAP_JS}"))
        .expect("extension keymap.js is missing");
    let en = js_string(&src, "EN");
    let ru = js_string(&src, "RU");
    assert_eq!(en.chars().count(), 94);
    assert_eq!(ru.chars().count(), 94);
    for (e, r) in en.chars().zip(ru.chars()) {
        assert_eq!(
            convert_text(&e.to_string(), Layout::En),
            r.to_string(),
            "EN {e:?} should map to RU {r:?}"
        );
        assert_eq!(
            convert_text(&r.to_string(), Layout::Ru),
            e.to_string(),
            "RU {r:?} should map to EN {e:?}"
        );
    }
}
