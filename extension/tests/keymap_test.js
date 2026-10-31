// Run with: gjs -m extension/tests/keymap_test.js
import System from 'system';
import {EN, RU, convertText, targetSource} from '../swiftswitcher@vipruk.github.io/keymap.js';

const cases = [
    ['ghbdtn', 'привет'],
    ['руддщ', 'hello'],
    ['Ghbdtn? rfr ltkf', 'Привет, как дела'],
    ['', ''],
    ['123 ,.', '123 бю'],
];

let failed = 0;
for (const [input, expected] of cases) {
    const actual = convertText(input);
    if (actual === expected) {
        print(`ok   ${JSON.stringify(input)} -> ${JSON.stringify(actual)}`);
    } else {
        failed++;
        printerr(`FAIL ${JSON.stringify(input)}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

for (const [input, expected] of [['ghbdtn', 'ru'], ['руддщ', 'us'], ['', 'ru'], ['Привет, world', 'us']]) {
    const actual = targetSource(input);
    if (actual === expected) {
        print(`ok   targetSource ${JSON.stringify(input)} -> ${actual}`);
    } else {
        failed++;
        printerr(`FAIL targetSource ${JSON.stringify(input)}: expected ${expected}, got ${actual}`);
    }
}

if ([...EN].length !== 94 || [...RU].length !== 94) {
    failed++;
    printerr('FAIL table rows must have 94 characters each');
}

if (failed > 0) {
    printerr(`${failed} failure(s)`);
    System.exit(1);
}
print('all keymap tests passed');
