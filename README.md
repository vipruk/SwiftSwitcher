# SwiftSwitcher

Automatic keyboard layout corrector for Linux, inspired by Punto Switcher.

You type `ghbdtn` because the layout was still English. SwiftSwitcher notices,
erases the word, types `привет` instead and switches the layout to Russian, so
you just keep typing. It works the other way too (`руддщ` → `hello`), and it
converts the selected text or the last word on demand.

- Languages: Russian and English (GNOME input sources `us` and `ru`).
- Current target: **Ubuntu 26.04, GNOME 50, Wayland**.
- Written in Rust. No network code. Nothing you type is logged.

> Status: early but usable every day on the target system. KDE, GNOME 46
> (Ubuntu 24.04) and X11 are planned, not done yet.

## Contents

- [Features](#features)
- [How it works](#how-it-works)
- [Install](#install)
- [First run](#first-run)
- [Using it](#using-it)
- [Learning and the dictionary](#learning-and-the-dictionary)
- [Configuration](#configuration)
- [Privacy](#privacy)
- [Troubleshooting](#troubleshooting)
- [Known limitations](#known-limitations)
- [Building from source](#building-from-source)
- [Project layout](#project-layout)
- [License](#license)

## Features

- **Automatic correction.** When you finish a word with a space and it was
  clearly typed in the wrong layout, it is replaced and the layout is switched.
  Case is kept: `Ghbdtn` → `Привет`, `GHBDTN` → `ПРИВЕТ`.
- **Careful by design.** A word is left alone when it is a real word in the
  layout it was typed in, is shorter than 3 letters, or looks like code or a
  path: anything with digits, `_`, `@`, `://`, `=`, `$`, `\`, `~`, a leading
  `-` or `/`, inner slashes or camelCase (`getUser`, `--force`, `~/src`,
  `https://x.ru`, `my_var`). Acronyms such as `NASDAQ` or `РСФСР` are kept
  too. On about 340,000 words of real Russian and English news text it makes
  no false corrections.
- **Undo.** Press `Pause` right after an automatic correction and it is undone,
  with the layout switched back.
- **Manual conversion** of the last word (`Pause`) or of the selected text
  (`Shift+Pause`).
- **Change case** of the selection (`Alt+Pause`): `пРИВЕТ мИР` → `Привет Мир`,
  for text typed with Caps Lock on by mistake.
- **It learns.** A word you undo three times is never corrected again. A word
  you convert by hand twice is corrected automatically from then on.
- **User dictionary.** "Never convert" and "Always convert" lists you can edit
  in the settings window or as plain text files.
- **Per-app rules.** No automatic correction in password managers (KeePassXC,
  Bitwarden, 1Password) or any app you add. Terminals and JetBrains IDEs get
  the right copy and paste shortcuts.
- **IT vocabulary.** Words like `gitlab`, `nginx`, `kubernetes`, `laravel` or
  `typescript` are recognized: `пшедфи` becomes `gitlab`.
- **Tray icon** with a menu, a **settings window** (GTK4 / libadwaita) in
  English or Russian, **start on login**, and `.deb` and AppImage packages.
- Config and dictionary changes made by hand are picked up within a second,
  without a restart.

## How it works

SwiftSwitcher has two parts:

1. **The daemon** (`swiftswitcher`), written in Rust. It reads key presses
   directly from the keyboard devices (`/dev/input/event*`) through evdev, in
   read-only mode: it never grabs the keyboard, so your keys always reach the
   apps normally. When a correction is needed, it types through its own virtual
   keyboard (`/dev/uinput`): Backspaces to erase the word, then the converted
   word.
2. **A small GNOME Shell extension** (`swiftswitcher@vipruk.github.io`). On
   Wayland, an ordinary program cannot see or change the current layout, the
   focused app or the selection, so the extension exposes exactly that over
   session D-Bus (`io.github.vipruk.SwiftSwitcher.Shell`): read and switch the
   input source, report the focused app id, and help convert the selection.

Deciding whether a word is wrong takes no network and no AI model: it uses
word lists and letter-trigram statistics for both languages. Each finished
word goes through these checks:

1. your "never" and "always" lists;
2. protected tokens (code, paths, URLs, acronyms; see above);
3. minimum length (3 letters);
4. is the word a real word in the layout it was typed in? Then keep it;
5. is its conversion a known word in the other language? Then correct it;
6. otherwise, correct only if the trigram score of the converted text is
   much better than that of the typed text and the word is at least 4 letters
   long.

The typed word is forgotten whenever the caret may have moved: Enter, Tab,
arrows, a mouse click, a key combination with Ctrl, Alt or Super, a long pause
(5 seconds by default), or key autorepeat. This is deliberate: a correction
applied in the wrong place would damage your text.

## Install

### .deb (Ubuntu 26.04)

Download `swiftswitcher_<version>_amd64.deb` from the
[releases page](https://github.com/vipruk/SwiftSwitcher/releases) and run:

    sudo apt install ./swiftswitcher_<version>_amd64.deb

The package installs the program, the GNOME extension (system-wide), the udev
rule for device access and a menu entry. It works on GNOME 50 only for now,
because the extension targets GNOME Shell 50.

Uninstall with `sudo apt remove swiftswitcher`. If you enabled autostart, also
remove `~/.config/autostart/io.github.vipruk.SwiftSwitcher.desktop`.

### AppImage

The AppImage uses GTK4 and libadwaita from the host system, so it needs a
distribution that ships them (Ubuntu 24.04 or newer). The desktop target is
still GNOME 50 on Ubuntu 26.04.

1. Download `SwiftSwitcher-<version>-x86_64.AppImage` from the releases page.
2. `chmod +x SwiftSwitcher-*-x86_64.AppImage`
3. Run it. The first run opens the settings window, which offers a `pkexec`
   step to install the udev rule and a button to install the GNOME extension.

## First run

1. Make sure you have exactly two input sources in GNOME Settings → Keyboard:
   **English (US)** and **Russian**.
2. Open **SwiftSwitcher** from the app grid. The settings window opens on the
   General page and runs the **setup check**:
   - keyboard access;
   - mouse or touchpad access;
   - virtual keyboard (`/dev/uinput`);
   - GNOME Shell extension;
   - input sources.

   Every line should say "Passed". A failed line has a **Fix** button where a
   fix is possible.
3. **Log out and back in once.** GNOME Shell loads a newly installed extension
   only at login.
4. The first time the settings window opens, it turns on **Start on login**,
   so SwiftSwitcher starts by itself after that. Turn the switch off on the
   General page if you don't want that.

After the relogin the tray icon appears in the top bar and correction is on.
You can check the setup from a terminal at any time:

    swiftswitcher check

Each line says `OK`, or `FAIL` with the fix.

**Turn off other layout switchers** (gswitch, other Punto-like tools, the
xkb-switch family) first, otherwise two programs will correct the same word.

## Using it

### Hotkeys

| Default | What it does |
| --- | --- |
| `Pause` (`Break`) | Convert the word you are typing, or the last word. Right after an automatic correction, before you type or click anything else, it **undoes** the correction instead. |
| `Shift+Pause` | Convert the selected text to the other layout, then switch to the layout the text is now in. |
| `Alt+Pause` | Invert the case of every letter in the selection (`пРИВЕТ` → `Привет`). The layout is not switched. |

All three can be changed on the Hotkeys page of the settings window: click
**Change**, press the new shortcut, or press Esc to cancel. No `Pause` key on
your laptop? Try `Ctrl+Alt+Space`. Shortcuts that would type text or clash with
common app shortcuts (`Ctrl+C` and similar) are refused.

Hotkeys also work in excluded apps; only automatic correction is off there.

Converting the selection works through the clipboard: SwiftSwitcher copies the
selection, converts it, pastes the result and restores what was in your
clipboard before.

### Tray menu

- **Auto-correction**: turn automatic correction on or off. Hotkeys keep
  working.
- **Convert selection**: the same as `Shift+Pause`.
- **Settings…**, **About**, **Quit**.

If something is wrong (the extension is not reachable, or the input sources
are not exactly US + Russian), the tray icon shows it.

### Settings window

Open it from the tray, from the app grid, or with `swiftswitcher settings`
(`--page general|hotkeys|dictionary|apps|about`).

- **General**: interface language (English or Russian), auto-correction, start
  on login, the background service status with a **Start** button, and the
  setup check.
- **Hotkeys**: the three shortcuts.
- **Dictionary**: the "Never convert" and "Always convert" lists.
- **Applications**: excluded applications. **Add running app…** lists the apps
  that are open now, so you don't have to look up app ids.
- **About**: version, website, privacy notes, licenses.

## Learning and the dictionary

- **Never convert.** Undo the same automatic correction three times
  (`learn_after_undos`) and the word is added to "Never convert". Words in this
  list are matched exactly as typed: if `ghbdtn` is there, typing `ghbdtn` is
  never corrected.
- **Always convert.** Convert the same word by hand with `Pause` twice
  (`learn_after_converts`) and it is added to "Always convert", so next time it
  is corrected automatically. This covers the words the statistics are not sure
  about, such as `github`, `mysql` or `php` typed in the Russian layout. A word
  that is itself a real word in the layout you typed it in is never learned, so
  converting `hello` by hand twice does not make `hello` turn into `руддщ`.
- Learning a word into one list removes it from the other.
- You can add and remove words on the Dictionary page or edit the files
  directly (one word per line):

      ~/.local/share/swiftswitcher/never.txt
      ~/.local/share/swiftswitcher/always.txt

## Configuration

Most settings are in the settings window. All of them live in
`~/.config/swiftswitcher/config.toml`. Every key is optional, and the file is
re-read within a second after you save it.

| Key | Default | Meaning |
| --- | --- | --- |
| `auto_correct` | `true` | Correct wrong-layout words automatically |
| `convert_last` | `"Pause"` | Hotkey: convert the last word / undo |
| `convert_selection` | `"Shift+Pause"` | Hotkey: convert the selection |
| `change_case` | `"Alt+Pause"` | Hotkey: change the case of the selection |
| `excluded_apps` | password managers | App ids where nothing is corrected automatically (hotkeys still work) |
| `terminal_apps` | common terminals | App ids that copy and paste with `Ctrl+Shift+C` / `Ctrl+Shift+V` |
| `idle_reset_secs` | `5` | Pause after which the typed word is forgotten |
| `settle_ms` | `40` | Wait after switching the layout before typing |
| `learn_after_undos` | `3` | Undos of the same word before it is never corrected again |
| `learn_after_converts` | `2` | Manual conversions of the same word before it is always corrected; `0` turns this off |
| `language` | `"en"` | Interface language: `"en"` or `"ru"` |

Example:

```toml
auto_correct = true
convert_last = "Ctrl+Alt+Space"
excluded_apps = ["org.keepassxc.KeePassXC", "bitwarden", "1password", "code"]
learn_after_converts = 3
language = "ru"
```

JetBrains IDEs (PhpStorm, IntelliJ IDEA, PyCharm and others) are recognized
automatically. In them the selection is copied with `Ctrl+Insert`, which works
in both the editor and the built-in terminal, where `Ctrl+C` would interrupt
the running command.

## Privacy

SwiftSwitcher reads all keystrokes locally in order to work. Nothing typed is
logged or sent anywhere: there is no network code. The only files it writes
are:

- its config (`~/.config/swiftswitcher/config.toml`);
- the never and always word lists (`~/.local/share/swiftswitcher/never.txt`
  and `always.txt`);
- `undos.txt` and `converts.txt` in the same directory, which record the words
  you undid or converted by hand and how often, until they are learned;
- the autostart entry while Start on login is on
  (`~/.config/autostart/io.github.vipruk.SwiftSwitcher.desktop`);
- an empty marker noting that autostart was set up
  (`~/.local/state/swiftswitcher/autostart-initialized`).

It reads all keyboard and pointer input, the id of the focused application,
and the clipboard only while converting a selection. The clipboard is then
restored, and its contents are never sent over D-Bus.

Device access is granted by a udev rule using the `uaccess` tag: the active
desktop session gets read access to your keyboards, mice and touchpads, and
write access to `/dev/uinput`. This is honest but broad: **any program running
in your active session can read those devices (see everything you type), and,
through `/dev/uinput`, create a virtual keyboard that types into any window,
not only SwiftSwitcher.** Remove `/etc/udev/rules.d/70-swiftswitcher.rules` to
undo it.

SwiftSwitcher never grabs devices; your keyboard keeps working normally even if
the daemon hangs or crashes.

## Troubleshooting

**Nothing is corrected.**
Run `swiftswitcher check`. The usual causes:
- you have not logged out and back in after installing, so the extension is
  not loaded;
- the input sources are not exactly English (US) and Russian;
- the udev rule is missing, so the keyboard or `/dev/uinput` cannot be opened;
- no mouse or touchpad can be read. Automatic correction stays off then,
  because a click could move the caret without SwiftSwitcher noticing. The
  log and `swiftswitcher check` say so.

**The last word before Enter was not corrected.**
That is expected. Correction happens when a word ends with a space; Enter
resets the word (see [Known limitations](#known-limitations)). Press `Pause`
before Enter to convert it by hand.

**A word keeps being "corrected" when it should not.**
Press `Pause` right after the correction to undo it. After three undos the word
is remembered. Or add it to "Never convert" on the Dictionary page.

**A word is never corrected.**
Convert it with `Pause` twice and it will be corrected automatically from then
on, or add it to "Always convert".

**No tray icon.**
GNOME needs an AppIndicator host for tray icons. Ubuntu ships the
"Ubuntu AppIndicators" extension, enabled by default. On other systems install
"AppIndicator and KStatusNotifierItem Support". The daemon waits for the host
and shows the icon once it appears.

**`Shift+Pause` does nothing in some app.**
The selection is copied with `Ctrl+C`, `Ctrl+Shift+C` in terminals, or
`Ctrl+Insert` in JetBrains IDEs. If your terminal is not in `terminal_apps`,
add its app id (on the Applications page, **Add running app…** shows it).

**Logs.**
Run the daemon in a terminal with more logging:

    swiftswitcher run          # or: RUST_LOG=debug swiftswitcher run

Logs never contain what you type.

## Known limitations

- **GNOME 50 on Wayland only** for now. KDE, GNOME 46 (Ubuntu 24.04) and X11
  are planned.
- **Only US English and Russian** layouts, exactly two input sources.
- **The word before Enter is not corrected automatically.** SwiftSwitcher does
  not grab the keyboard, so the Enter key has already reached the app (and the
  message may already be sent) by the time the word could be checked.
- **Selection commands rely on the clipboard and the copy shortcut.** Apps that
  handle mouse selection themselves inside a terminal (for example a TUI with
  its own selection) may not work with `Shift+Pause` and `Alt+Pause`.
- Some short or ambiguous words are not corrected automatically. Teach them
  with two manual conversions.

## Building from source

Requirements: Rust 1.99.1 (installed automatically by `rustup` from
`rust-toolchain.toml`) and these packages:

    sudo apt install pkg-config libdbus-1-dev libgtk-4-dev libadwaita-1-dev

Build and run tests:

    cargo build --release
    cargo test --workspace
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings

Run a single test:

    cargo test -p swift-engine detect::tests::protected_tokens
    cargo test -p swiftswitcher corrector

The tray test needs its own session bus:

    dbus-run-session -- cargo test -p swiftswitcher -- --ignored tray_waits_for_watcher

### Development install

1. Run `scripts/dev-setup.sh`. It uses `sudo` to install the udev rule
   (`packaging/udev/70-swiftswitcher.rules`), reloads udev so device access
   applies immediately, and links and enables the GNOME extension from the
   working tree.
2. Log out and back in. Only the GNOME extension needs this.
3. Check the setup: `cargo run -p swiftswitcher -- check`. Every line must say
   `OK`; a `FAIL` line names the fix.
4. Start the daemon: `cargo run -p swiftswitcher -- run` (set `RUST_LOG=debug`
   for more logging; typed content is never logged).

### Measuring accuracy

The detector is checked against real text from the Leipzig Corpora Collection
(news, CC BY 4.0). The data is downloaded for local testing only and is never
committed:

    scripts/fetch-corpus.sh
    cargo test --release -p swift-engine --test corpus -- --ignored --nocapture

It reports false corrections (correct words that would be changed; this must
be 0) and recall (how many words typed in the wrong layout are caught).

### Packages

    cargo install cargo-deb --locked
    cargo deb -p swiftswitcher               # target/debian/*.deb
    packaging/appimage/build.sh              # needs appimagetool in $APPIMAGETOOL

CI (GitHub Actions) runs formatting, clippy, the tests and the desktop file
checks on every push, builds both packages, and attaches them to a GitHub
release for `v*` tags.

## Project layout

```
crates/engine/     swift-engine: layout-independent logic, no I/O
  src/keymap.rs      key codes ↔ characters for the US and Russian layouts
  src/tracker.rs     turns key events into "word finished" and hotkey events
  src/detect.rs      decides whether a word was typed in the wrong layout
  src/model.rs       word lists and trigram scoring
  src/dict.rs        user never/always lists and learning
  src/hotkey.rs      hotkey parsing and safety checks
  data/              word lists and trigram tables (see data/LICENSE.md)
crates/daemon/     swiftswitcher: the program
  src/input.rs       reading evdev devices
  src/inject.rs      the uinput virtual keyboard
  src/desktop.rs     D-Bus client for the GNOME Shell extension
  src/corrector.rs   correction, undo, selection commands, learning
  src/tray.rs        tray icon (StatusNotifierItem)
  src/settings/      GTK4 / libadwaita settings window
  src/check.rs       `swiftswitcher check`
extension/         GNOME Shell extension (JavaScript)
packaging/         udev rule, Debian scripts, AppImage build
data/              desktop entry, icon, AppStream metainfo
scripts/           development setup, corpus download
```

## Contributing

Bug reports and pull requests are welcome. When reporting a wrong correction,
include the word as typed, the layout it was typed in and what it became. If
you send a change to detection, run the corpus test above: false corrections
must stay at 0.

To report a security problem, use
[private vulnerability reporting](https://github.com/vipruk/SwiftSwitcher/security/advisories/new)
instead of a public issue.

## License

The code is MIT (see `LICENSE`). The bundled word and trigram data in
`crates/engine/data/` is derived from third-party lists and is licensed
CC-BY-SA-4.0, except the curated `en.tech.words`, which is MIT; see
`crates/engine/data/LICENSE.md`.
