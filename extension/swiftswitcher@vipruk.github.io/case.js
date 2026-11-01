// Pure case inversion for the "change case of selection" command; no GNOME
// imports, so it is unit-tested with plain gjs.

const single = s => [...s].length === 1;

// Inverts the case of every letter: `пРИВЕТ` becomes `Привет`. Characters
// without a one-to-one case mapping (digits, punctuation, caseless scripts,
// and letters like `ß` that would expand) are kept as they are.
export function invertCase(text) {
    let out = '';
    for (const c of text) {
        const up = c.toUpperCase();
        const low = c.toLowerCase();
        if (c !== up && single(up))
            out += up;
        else if (c !== low && single(low))
            out += low;
        else
            out += c;
    }
    return out;
}
