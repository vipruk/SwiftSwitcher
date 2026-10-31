# SwiftSwitcher

Automatic keyboard layout corrector for Linux, inspired by Punto Switcher. It
notices when a word was typed in the wrong layout (for example `ghbdtn` instead
of `привет`) and replaces it with the right one. Supports the RU and EN layouts
(GNOME input sources `us` and `ru`). Current target: Ubuntu 26.04, GNOME 50,
Wayland.

It has two parts: a Rust daemon (`swiftswitcher`) that watches typing and types
the correction, and a small GNOME Shell extension that exposes the input source,
the focused app and the selection over D-Bus.

## Privacy

SwiftSwitcher reads all keystrokes locally in order to work. Nothing typed is
logged, stored or sent anywhere: there is no network code. The only files it
writes are its config and the dictionary words you explicitly teach it.

Device access is granted by a udev rule using the `uaccess` tag: the active
desktop session gets read access to your keyboards, mice and touchpads, and write
access to `/dev/uinput`. This is honest but broad: **any program running in your
active session can read those devices, not only SwiftSwitcher.** Remove
`/etc/udev/rules.d/70-swiftswitcher.rules` to undo it.

SwiftSwitcher never grabs devices; your keyboard keeps working normally.

## Development install

1. Run `scripts/dev-setup.sh`. It uses `sudo` to install the udev rule
   (`packaging/udev/70-swiftswitcher.rules`) and reloads udev, so device access
   applies immediately. It also links and enables the GNOME extension.
2. Log out and back in. Only the GNOME extension needs this.
3. Check the setup: `cargo run -p swiftswitcher -- check`. Every line must say `OK`;
   a `FAIL` line names the fix.
4. Start the daemon: `cargo run -p swiftswitcher -- run` (set `RUST_LOG=debug`
   for more logging; typed content is never logged).

Stop other layout switchers (gswitch, Punto-like tools, etc.) before running
SwiftSwitcher, otherwise both will correct the same word.

## Hotkeys

Defaults, configurable:

- `Pause`: convert the last word (press again to undo an automatic correction).
- `Shift+Pause`: convert the selected text between layouts.

## Configuration

`~/.config/swiftswitcher/config.toml`; all keys are optional.

| Key | Default | Meaning |
| --- | --- | --- |
| `auto_correct` | `true` | Correct wrong-layout words automatically |
| `convert_last` | `"Pause"` | Hotkey: convert the last word |
| `convert_selection` | `"Shift+Pause"` | Hotkey: convert the selection |
| `excluded_apps` | password managers | App ids where nothing is corrected |
| `terminal_apps` | common terminals | App ids that use terminal paste/copy shortcuts |
| `idle_reset_secs` | `5` | Pause after which the typed word is forgotten |
| `settle_ms` | `40` | Wait after switching the layout before typing |

Dictionary words you teach it live in `~/.local/share/swiftswitcher/never.txt`
and `always.txt`.

## License

The code is MIT (see `LICENSE`). The bundled word and trigram data in
`crates/engine/data/` is derived from third-party lists and is licensed
CC-BY-SA-4.0; see `crates/engine/data/LICENSE.md`.
