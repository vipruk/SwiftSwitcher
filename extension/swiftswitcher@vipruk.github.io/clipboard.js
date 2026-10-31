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

/**
 * True for a single line ending in a newline: what JetBrains IDEs and VS Code
 * copy for Ctrl+C with nothing selected (the whole current line). Such text is
 * not converted, so a converted duplicate of the line is never pasted.
 *
 * @param {string} text
 * @returns {boolean}
 */
export function isWholeLineCopy(text) {
    return text.endsWith('\n') && text.indexOf('\n') === text.length - 1;
}

/**
 * Saves the clipboard for one selection-convert cycle and restores it.
 *
 * `io` does the actual clipboard and timer work:
 *   mimetypes() -> string[] | null
 *   read(mimetype) -> Promise<bytes | null>
 *   clear()                      sets the clipboard to ''
 *   restore(mimetype, bytes)
 *   addTimeout(seconds, fn) -> id, removeTimeout(id)
 *
 * The saved copy is dropped after `timeoutSeconds` even if end() never comes.
 * end() (or destroy()) while begin() is still reading cancels that begin():
 * it then neither saves nor clears, so a late begin() can never overwrite
 * the user's clipboard with '' after the cycle is over.
 */
export class ClipboardSaver {
    constructor(io, timeoutSeconds) {
        this._io = io;
        this._timeoutSeconds = timeoutSeconds;
        // {mimetype, bytes}, mimetype null for an empty clipboard; null when
        // nothing is saved.
        this._saved = null;
        this._timeoutId = 0;
        this._generation = 0;
    }

    /** @returns {Promise<boolean>} whether the clipboard was saved and cleared */
    async begin() {
        const generation = this._generation;
        // A repeated begin within the same cycle must not overwrite the
        // original with our own ''.
        if (this._saved === null) {
            const mimetype = pickMimetype(this._io.mimetypes());
            const bytes = mimetype ? await this._io.read(mimetype) : null;
            if (generation !== this._generation)
                return false;
            // Another begin may have saved while we were waiting.
            if (this._saved === null)
                this._store(bytes ? {mimetype, bytes} : {mimetype: null, bytes: null});
        }
        this._io.clear();
        return true;
    }

    /** @returns {boolean} whether something saved was handled */
    end() {
        this._generation++;
        if (this._saved === null)
            return false;
        const {mimetype, bytes} = this._saved;
        this._drop();
        // An originally empty clipboard is left as it is.
        if (mimetype)
            this._io.restore(mimetype, bytes);
        return true;
    }

    destroy() {
        this._generation++;
        this._drop();
    }

    _store(saved) {
        this._saved = saved;
        this._timeoutId = this._io.addTimeout(this._timeoutSeconds, () => {
            this._timeoutId = 0;
            this._saved = null;
        });
    }

    _drop() {
        if (this._timeoutId) {
            this._io.removeTimeout(this._timeoutId);
            this._timeoutId = 0;
        }
        this._saved = null;
    }
}
