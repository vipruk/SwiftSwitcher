// Run with: gjs -m extension/tests/case_test.js
import System from 'system';
import {invertCase} from '../swiftswitcher@vipruk.github.io/case.js';

let failed = 0;
function check(name, actual, expected) {
    if (actual === expected) {
        print(`ok   ${name}`);
    } else {
        failed++;
        printerr(`FAIL ${name}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
    }
}

check('ru mixed', invertCase('пРИВЕТ'), 'Привет');
check('en mixed', invertCase('hELLO'), 'Hello');
check('both scripts', invertCase('Hello Мир'), 'hELLO мИР');
check('yo', invertCase('ёЖ Ёж'), 'Ёж ёЖ');
check('digits and punctuation', invertCase('a1, B2! (c)'), 'A1, b2! (C)');
check('caseless letters kept', invertCase('日本語 ß'), '日本語 ß');
check('empty', invertCase(''), '');
check('newlines', invertCase('aB\nCd'), 'Ab\ncD');

if (failed > 0)
    System.exit(1);
