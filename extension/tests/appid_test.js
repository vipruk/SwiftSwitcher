// Run with: gjs -m extension/tests/appid_test.js
import System from 'system';
import {appIdOf, uniqueAppIds} from '../swiftswitcher@vipruk.github.io/appid.js';

let failed = 0;
function check(name, actual, expected) {
    if (JSON.stringify(actual) === JSON.stringify(expected)) {
        print(`ok   ${name} -> ${JSON.stringify(actual)}`);
    } else {
        failed++;
        printerr(`FAIL ${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

check('desktop app', appIdOf('org.gnome.Terminal.desktop', false, 'Gnome-terminal'), 'org.gnome.Terminal');
check('no suffix', appIdOf('firefox', false, 'x'), 'firefox');
check('window-backed uses wm class', appIdOf('window:42', true, 'jetbrains-idea'), 'jetbrains-idea');
check('window-backed, no class', appIdOf('window:42', true, undefined), '');
check('no app uses wm class', appIdOf(null, false, 'Foo'), 'Foo');
check('nothing', appIdOf(null, false, null), '');
check('empty id falls back', appIdOf('', false, 'Bar'), 'Bar');
check('unique', uniqueAppIds(['a', '', 'b', 'a', null, 'c']), ['a', 'b', 'c']);

if (failed > 0)
    System.exit(1);
