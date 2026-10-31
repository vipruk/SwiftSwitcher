// Run with: gjs -m extension/tests/clipboard_test.js
import System from 'system';
import {ClipboardSaver, isWholeLineCopy, pickMimetype} from '../swiftswitcher@vipruk.github.io/clipboard.js';

let failed = 0;
function check(name, actual, expected) {
    if (JSON.stringify(actual) === JSON.stringify(expected)) {
        print(`ok   ${name} -> ${JSON.stringify(actual)}`);
    } else {
        failed++;
        printerr(`FAIL ${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

const mimeCases = [
    [null, null],
    [[], null],
    [['image/png', 'text/plain', 'text/plain;charset=utf-8'], 'text/plain;charset=utf-8'],
    [['TARGETS', 'UTF8_STRING', 'STRING'], 'UTF8_STRING'],
    [['text/html', 'text/plain'], 'text/plain'],
    [['image/png', 'image/jpeg'], 'image/png'],
    [['x-special/gnome-copied-files', 'text/uri-list'], 'x-special/gnome-copied-files'],
];
for (const [input, expected] of mimeCases)
    check(`pickMimetype ${JSON.stringify(input)}`, pickMimetype(input), expected);

const lineCases = [
    ['    let x = 1;\n', true],
    ['\n', true],
    ['line\r\n', true],
    ['ghbdtn', false],
    ['ghbdtn\nrfr', false],
    ['two\nlines\n', false],
    ['\nleading', false],
    ['', false],
];
for (const [input, expected] of lineCases)
    check(`isWholeLineCopy ${JSON.stringify(input)}`, isWholeLineCopy(input), expected);

// A fake clipboard whose reads complete only when the test says so.
function fakeIo() {
    const io = {
        clipboard: {mimetype: 'text/plain', bytes: 'original'},
        pendingReads: [],
        timeouts: new Map(),
        nextId: 1,
        mimetypes: () => (io.clipboard.mimetype ? [io.clipboard.mimetype] : []),
        read: mimetype => new Promise(resolve =>
            io.pendingReads.push(() => resolve(io.clipboard.mimetype === mimetype ? io.clipboard.bytes : null))),
        clear: () => {
            io.clipboard = {mimetype: 'text/plain', bytes: ''};
        },
        restore: (mimetype, bytes) => {
            io.clipboard = {mimetype, bytes};
        },
        addTimeout: (_seconds, fn) => {
            const id = io.nextId++;
            io.timeouts.set(id, fn);
            return id;
        },
        removeTimeout: id => io.timeouts.delete(id),
        finishReads: () => {
            for (const r of io.pendingReads.splice(0))
                r();
        },
    };
    return io;
}

// Normal cycle: begin saves and clears, end restores.
{
    const io = fakeIo();
    const saver = new ClipboardSaver(io, 5);
    const begun = saver.begin();
    io.finishReads();
    check('cycle: begin', await begun, true);
    check('cycle: cleared', io.clipboard.bytes, '');
    io.clipboard = {mimetype: 'text/plain', bytes: 'converted'};
    check('cycle: end', saver.end(), true);
    check('cycle: restored', io.clipboard.bytes, 'original');
    check('cycle: timer removed', io.timeouts.size, 0);
}

// End arrives while begin is still reading (the daemon's Begin timed out):
// the late begin must neither save nor clear the clipboard.
{
    const io = fakeIo();
    const saver = new ClipboardSaver(io, 5);
    const begun = saver.begin();
    check('late begin: end first', saver.end(), false);
    io.finishReads();
    check('late begin: result', await begun, false);
    check('late begin: clipboard untouched', io.clipboard.bytes, 'original');
    check('late begin: nothing saved', io.timeouts.size, 0);
}

// A repeated begin in one cycle keeps the original, not our ''.
{
    const io = fakeIo();
    const saver = new ClipboardSaver(io, 5);
    const first = saver.begin();
    io.finishReads();
    await first;
    check('repeat: second begin', await saver.begin(), true);
    saver.end();
    check('repeat: original restored', io.clipboard.bytes, 'original');
}

// The save timeout drops the copy; a later end restores nothing.
{
    const io = fakeIo();
    const saver = new ClipboardSaver(io, 5);
    const begun = saver.begin();
    io.finishReads();
    await begun;
    for (const fn of io.timeouts.values())
        fn();
    io.clipboard = {mimetype: 'text/plain', bytes: 'user copied this'};
    check('timeout: end', saver.end(), false);
    check('timeout: user clipboard kept', io.clipboard.bytes, 'user copied this');
}

// An empty clipboard stays empty.
{
    const io = fakeIo();
    io.clipboard = {mimetype: null, bytes: null};
    const saver = new ClipboardSaver(io, 5);
    check('empty: begin', await saver.begin(), true);
    check('empty: end', saver.end(), true);
    check('empty: not restored', io.clipboard.bytes, '');
}

if (failed > 0) {
    printerr(`${failed} failure(s)`);
    System.exit(1);
}
print('all clipboard tests passed');
