// Pure clipboard helpers (no GNOME imports), tested with
// `gjs -m extension/tests/clipboard_test.js`.

// Text mimetypes in order of preference.
const TEXT_MIMETYPES = [
    'text/plain;charset=utf-8',
    'UTF8_STRING',
    'text/plain',
    'STRING',
    'TEXT',
];

/**
 * Picks the mimetype to save so that the clipboard can be restored later:
 * the preferred text type if one is offered, otherwise the first offered type
 * (e.g. image/png or text/uri-list). Returns null for an empty clipboard.
 *
 * @param {string[] | null} mimetypes
 * @returns {string | null}
 */
export function pickMimetype(mimetypes) {
    if (!mimetypes || mimetypes.length === 0)
        return null;
    return TEXT_MIMETYPES.find(t => mimetypes.includes(t)) ?? mimetypes[0];
}
