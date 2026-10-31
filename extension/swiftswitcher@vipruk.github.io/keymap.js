// US <-> RU keymap by physical key. Copied from crates/engine/src/keymap.rs
// (unshifted row followed by shifted row); crates/engine/tests/extension_keymap.rs
// keeps the two in sync. This module is pure: no GNOME imports, so it can be
// tested with `gjs -m extension/tests/keymap_test.js`.

export const EN = "`1234567890-=qwertyuiop[]asdfghjkl;'\\zxcvbnm,./~!@#$%^&*()_+QWERTYUIOP{}ASDFGHJKL:\"|ZXCVBNM<>?";
export const RU = "ё1234567890-=йцукенгшщзхъфывапролджэ\\ячсмитьбю.Ё!\"№;%:?*()_+ЙЦУКЕНГШЩЗХЪФЫВАПРОЛДЖЭ/ЯЧСМИТЬБЮ,";

const EN_CHARS = [...EN];
const RU_CHARS = [...RU];
const CYRILLIC = /[а-яА-ЯёЁ]/;
const LATIN = /[A-Za-z]/;

function isMostlyCyrillic(text) {
    let cyrillic = 0;
    let latin = 0;
    for (const c of text) {
        if (CYRILLIC.test(c))
            cyrillic++;
        else if (LATIN.test(c))
            latin++;
    }
    return cyrillic > latin;
}

/**
 * The input source id that `convertText(text)` converts into: "us" for
 * RU->EN, "ru" for EN->RU.
 *
 * @param {string} text
 * @returns {string}
 */
export function targetSource(text) {
    return isMostlyCyrillic(text) ? 'us' : 'ru';
}

/**
 * Re-types `text` as if the other layout had been active. Direction: more
 * Cyrillic than Latin letters means RU->EN, otherwise EN->RU. Unmapped
 * characters pass through unchanged.
 *
 * @param {string} text
 * @returns {string}
 */
export function convertText(text) {
    const [from, to] = isMostlyCyrillic(text) ? [RU_CHARS, EN_CHARS] : [EN_CHARS, RU_CHARS];
    let out = '';
    for (const c of text) {
        const i = from.indexOf(c);
        out += i < 0 ? c : to[i];
    }
    return out;
}
