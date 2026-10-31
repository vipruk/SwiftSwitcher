// Run with: gjs -m extension/tests/clipboard_test.js
import System from 'system';
import {pickMimetype} from '../swiftswitcher@vipruk.github.io/clipboard.js';

const cases = [
    [null, null],
    [[], null],
    [['image/png', 'text/plain', 'text/plain;charset=utf-8'], 'text/plain;charset=utf-8'],
    [['TARGETS', 'UTF8_STRING', 'STRING'], 'UTF8_STRING'],
    [['text/html', 'text/plain'], 'text/plain'],
    [['image/png', 'image/jpeg'], 'image/png'],
    [['x-special/gnome-copied-files', 'text/uri-list'], 'x-special/gnome-copied-files'],
];

let failed = 0;
for (const [input, expected] of cases) {
    const actual = pickMimetype(input);
    if (actual === expected) {
        print(`ok   ${JSON.stringify(input)} -> ${JSON.stringify(actual)}`);
    } else {
        failed++;
        printerr(`FAIL ${JSON.stringify(input)}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

if (failed > 0) {
    printerr(`${failed} failure(s)`);
    System.exit(1);
}
print('all clipboard tests passed');
