// SwiftSwitcher GNOME Shell extension: a session D-Bus bridge for the
// SwiftSwitcher daemon. Clipboard text never leaves this process (it is
// not returned over D-Bus) and is never logged.

import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import St from 'gi://St';

import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Keyboard from 'resource:///org/gnome/shell/ui/status/keyboard.js';
import * as KeyboardManager from 'resource:///org/gnome/shell/misc/keyboardManager.js';

import {appIdOf, uniqueAppIds} from './appid.js';
import {invertCase} from './case.js';
import {ClipboardSaver, isWholeLineCopy} from './clipboard.js';
import {convertText, targetSource} from './keymap.js';

const BUS_NAME = 'io.github.vipruk.SwiftSwitcher.Shell';
const OBJECT_PATH = '/io/github/vipruk/SwiftSwitcher/Shell';

const IFACE_XML = `
<node>
  <interface name="io.github.vipruk.SwiftSwitcher.Shell">
    <method name="GetLayout">
      <arg type="s" direction="out" name="id"/>
    </method>
    <method name="ListLayouts">
      <arg type="as" direction="out" name="ids"/>
    </method>
    <method name="SetLayout">
      <arg type="s" direction="in" name="id"/>
      <arg type="b" direction="out" name="ok"/>
    </method>
    <method name="GetFocusedApp">
      <arg type="s" direction="out" name="app"/>
    </method>
    <method name="ListRunningApps">
      <arg type="as" direction="out" name="apps"/>
    </method>
    <method name="SelectionBegin">
      <arg type="b" direction="out" name="ok"/>
    </method>
    <method name="SelectionConvert">
      <arg type="s" direction="out" name="target"/>
    </method>
    <method name="SelectionChangeCase">
      <arg type="b" direction="out" name="changed"/>
    </method>
    <method name="SelectionEnd">
      <arg type="b" direction="out" name="ok"/>
    </method>
  </interface>
</node>`;

const CLIPBOARD = St.ClipboardType.CLIPBOARD;

// Saved clipboard contents are dropped after this many seconds even if
// SelectionEnd never arrives (e.g. the daemon died mid-cycle).
const SAVE_TIMEOUT_SECONDS = 5;

function readClipboard() {
    return new Promise(resolve => {
        St.Clipboard.get_default().get_text(CLIPBOARD, (_clipboard, text) => resolve(text));
    });
}

// The GBytes handed to the callback is only borrowed and is freed after it
// returns, so its data is copied out. Restoring a freed GBytes stalls the
// shell for seconds and loses the clipboard. Empty content counts as nothing.
function readContent(mimetype) {
    return new Promise(resolve => {
        St.Clipboard.get_default().get_content(CLIPBOARD, mimetype, (_clipboard, bytes) => {
            const data = bytes?.get_size() ? bytes.toArray().slice() : null;
            resolve(data);
        });
    });
}

function writeClipboard(text) {
    St.Clipboard.get_default().set_text(CLIPBOARD, text);
}

const CLIPBOARD_IO = {
    mimetypes: () => St.Clipboard.get_default().get_mimetypes(CLIPBOARD),
    read: readContent,
    clear: () => writeClipboard(''),
    restore: (mimetype, data) =>
        St.Clipboard.get_default().set_content(CLIPBOARD, mimetype, new GLib.Bytes(data)),
    addTimeout: (seconds, fn) => GLib.timeout_add_seconds(
        GLib.PRIORITY_DEFAULT, seconds, () => {
            fn();
            return GLib.SOURCE_REMOVE;
        }),
    removeTimeout: id => GLib.source_remove(id),
};

function returnBoolean(invocation, value) {
    invocation.return_value(new GLib.Variant('(b)', [value]));
}

function returnString(invocation, value) {
    invocation.return_value(new GLib.Variant('(s)', [value]));
}

class ShellBridge {
    constructor() {
        this._saver = new ClipboardSaver(CLIPBOARD_IO, SAVE_TIMEOUT_SECONDS);
    }

    destroy() {
        this._saver.destroy();
    }

    _sources() {
        const manager = Keyboard.getInputSourceManager();
        return Object.values(manager.inputSources).sort((a, b) => a.index - b.index);
    }

    // The current input source, but only once mutter has applied its keymap
    // layout group; '' while a switch is still pending. The daemon types
    // right after this confirms a switch, so it must not report the source
    // too early: InputSourceManager.currentSource changes synchronously on
    // activate(), while the backend applies the group asynchronously.
    //
    // GNOME 50 exposes no getter for the applied group (Meta.Backend has
    // set_keymap_async() and the keymap-layout-group-changed signal only), so
    // this uses KeyboardManager's `_current`, which it assigns after
    // `await global.backend.set_keymap_async(...)`, i.e. after the backend
    // applied the group. Measured in a nested GNOME Shell 50.1: currentSource
    // flips immediately, keymap-layout-group-changed follows 3-18 ms later and
    // `_current` right after it. If that private field ever disappears this
    // returns '' and the daemon does nothing (it never types unconfirmed).
    GetLayout() {
        const source = Keyboard.getInputSourceManager().currentSource;
        if (!source)
            return '';
        const keyboardManager = KeyboardManager.getKeyboardManager();
        if (!('_current' in keyboardManager)) {
            if (!this._warnedNoCurrent) {
                this._warnedNoCurrent = true;
                console.error('SwiftSwitcher: cannot confirm the applied layout; corrections disabled');
            }
            return '';
        }
        return keyboardManager._current?.id === source.xkbId ? source.id : '';
    }

    ListLayouts() {
        return this._sources().map(source => source.id);
    }

    SetLayout(id) {
        const source = this._sources().find(s => s.id === id);
        if (!source)
            return false;
        source.activate(true);
        return true;
    }

    GetFocusedApp() {
        const app = Shell.WindowTracker.get_default().focus_app;
        return appIdOf(app?.get_id(), app?.is_window_backed() ?? false,
            global.display.focus_window?.get_wm_class());
    }

    ListRunningApps() {
        const apps = Shell.AppSystem.get_default().get_running();
        return uniqueAppIds(apps.map(app => appIdOf(
            app.get_id(), app.is_window_backed(), app.get_windows()[0]?.get_wm_class())));
    }

    async SelectionBeginAsync(_params, invocation) {
        try {
            returnBoolean(invocation, await this._saver.begin());
        } catch (e) {
            console.error(`SwiftSwitcher: SelectionBegin failed: ${e.name}`);
            returnBoolean(invocation, false);
        }
    }

    // Returns the input source id the converted text is in, or '' if
    // nothing was converted (then the daemon does not paste).
    async SelectionConvertAsync(_params, invocation) {
        try {
            const text = await readClipboard();
            // Nothing selected: empty, or an editor copied the whole line.
            if (!text || isWholeLineCopy(text)) {
                returnString(invocation, '');
                return;
            }
            writeClipboard(convertText(text));
            returnString(invocation, targetSource(text));
        } catch (e) {
            console.error(`SwiftSwitcher: SelectionConvert failed: ${e.name}`);
            returnString(invocation, '');
        }
    }

    // Inverts the case of the copied text in place; the layout is not
    // switched. Returns false if there was nothing to change (then the
    // daemon does not paste).
    async SelectionChangeCaseAsync(_params, invocation) {
        try {
            const text = await readClipboard();
            if (!text || isWholeLineCopy(text)) {
                returnBoolean(invocation, false);
                return;
            }
            writeClipboard(invertCase(text));
            returnBoolean(invocation, true);
        } catch (e) {
            console.error(`SwiftSwitcher: SelectionChangeCase failed: ${e.name}`);
            returnBoolean(invocation, false);
        }
    }

    SelectionEnd() {
        return this._saver.end();
    }
}

export default class SwiftSwitcherExtension extends Extension {
    enable() {
        this._bridge = new ShellBridge();
        this._dbus = Gio.DBusExportedObject.wrapJSObject(IFACE_XML, this._bridge);
        this._dbus.export(Gio.DBus.session, OBJECT_PATH);
        this._ownerId = Gio.bus_own_name_on_connection(
            Gio.DBus.session, BUS_NAME, Gio.BusNameOwnerFlags.NONE, null, null);
    }

    disable() {
        if (this._ownerId) {
            Gio.bus_unown_name(this._ownerId);
            this._ownerId = 0;
        }
        this._dbus?.unexport();
        this._dbus = null;
        this._bridge?.destroy();
        this._bridge = null;
    }
}
