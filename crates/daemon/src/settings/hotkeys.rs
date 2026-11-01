//! The Hotkeys page: the two commands, their bindings and a capture dialog.

use super::{Page, Shell, new_page, update_config};
use crate::config::{Config, config_path};
use crate::i18n::{Lang, Msg, tr};
use adw::prelude::*;
use gtk::{gdk, glib};
use std::rc::Rc;
use swift_engine::hotkey::{Hotkey, key_name};

/// Modifier keys held during a key press.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CaptureError {
    ModifierOnly,
    Unsupported,
}

/// evdev codes of Ctrl, Shift, Alt and Super keys, both sides.
const MODIFIER_CODES: [u16; 8] = [29, 97, 42, 54, 56, 100, 125, 126];

/// Maps a GDK hardware keycode (evdev code + 8) and the modifier state to
/// a hotkey. Pure, so it is testable without GTK.
pub(super) fn capture(keycode: u32, mods: Mods) -> Result<Hotkey, CaptureError> {
    let code = keycode
        .checked_sub(8)
        .and_then(|c| u16::try_from(c).ok())
        .ok_or(CaptureError::Unsupported)?;
    if MODIFIER_CODES.contains(&code) {
        return Err(CaptureError::ModifierOnly);
    }
    key_name(code).ok_or(CaptureError::Unsupported)?;
    Ok(Hotkey {
        code,
        shift: mods.shift,
        ctrl: mods.ctrl,
        alt: mods.alt,
        meta: mods.meta,
    })
}

/// Whether `new` equals the binding `other` (a config string).
pub(super) fn conflicts(new: &Hotkey, other: &str) -> bool {
    Hotkey::parse(other).is_ok_and(|o| o == *new)
}

/// Whether resetting to `default` would duplicate the other command's binding.
fn reset_conflicts(default: &str, other: &str) -> bool {
    Hotkey::parse(default).is_ok_and(|h| conflicts(&h, other))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Command {
    Last,
    Selection,
}

impl Command {
    const ALL: [Command; 2] = [Command::Last, Command::Selection];

    fn title(self) -> Msg {
        match self {
            Command::Last => Msg::HotkeyLastHint,
            Command::Selection => Msg::ConvertSelection,
        }
    }

    fn hint(self) -> Option<Msg> {
        match self {
            Command::Last => None,
            Command::Selection => Some(Msg::HotkeySelectionHint),
        }
    }

    fn field(self, c: &mut Config) -> &mut String {
        match self {
            Command::Last => &mut c.convert_last,
            Command::Selection => &mut c.convert_selection,
        }
    }

    fn get(self, c: &Config) -> &str {
        match self {
            Command::Last => &c.convert_last,
            Command::Selection => &c.convert_selection,
        }
    }

    fn other(self) -> Command {
        match self {
            Command::Last => Command::Selection,
            Command::Selection => Command::Last,
        }
    }
}

fn fresh_config() -> Config {
    Config::load_or_default(&config_path()).unwrap_or_else(|e| {
        log::warn!("{e:#}");
        Config::default()
    })
}

/// One command's row and the widgets that follow its binding.
struct CommandRow {
    command: Command,
    binding: gtk::Label,
    reset: gtk::Button,
}

impl CommandRow {
    fn refresh(&self) {
        let c = fresh_config();
        self.binding.set_label(self.command.get(&c));
        let mut default = Config::default();
        let default = self.command.field(&mut default).clone();
        self.reset.set_sensitive(self.command.get(&c) != default);
    }
}

pub(super) fn page(shell: &Shell, lang: Lang) -> adw::PreferencesPage {
    let page = new_page(Page::Hotkeys, lang);
    let group = adw::PreferencesGroup::new();
    let mut rows = Vec::new();
    for command in Command::ALL {
        let row = adw::ActionRow::builder()
            .title(tr(lang, command.title()))
            .build();
        if let Some(h) = command.hint() {
            row.set_subtitle(tr(lang, h));
        }
        let binding = gtk::Label::builder()
            .css_classes(["monospace"])
            .valign(gtk::Align::Center)
            .build();
        let change = gtk::Button::builder()
            .label(tr(lang, Msg::Change))
            .valign(gtk::Align::Center)
            .build();
        let reset = gtk::Button::builder()
            .label(tr(lang, Msg::Reset))
            .valign(gtk::Align::Center)
            .build();
        row.add_suffix(&binding);
        row.add_suffix(&change);
        row.add_suffix(&reset);
        group.add(&row);
        rows.push(Rc::new(CommandRow {
            command,
            binding,
            reset: reset.clone(),
        }));
        let r = rows.last().cloned().unwrap();
        let all = rows.clone();
        r.refresh();

        let s = shell.clone();
        let this = r.clone();
        reset.connect_clicked(move |_| {
            let mut default = Config::default();
            let value = command.field(&mut default).clone();
            if reset_conflicts(&value, command.other().get(&fresh_config())) {
                s.toast(tr(lang, Msg::ErrDuplicate));
                return;
            }
            if let Err(e) = update_config(|c| *command.field(c) = value) {
                log::warn!("cannot save the hotkey: {e:#}");
                s.toast(tr(lang, Msg::SaveFailed));
            }
            this.refresh();
        });
        let s = shell.clone();
        change.connect_clicked(move |b| {
            let parent = b.root().and_then(|r| r.dynamic_cast::<gtk::Widget>().ok());
            let rows = all.clone();
            open_capture(&s, lang, command, parent, move || {
                rows.iter().for_each(|r| r.refresh());
            });
        });
    }
    page.add(&group);

    let tip = adw::PreferencesGroup::new();
    let label = gtk::Label::builder()
        .label(tr(lang, Msg::HotkeysTip))
        .wrap(true)
        .xalign(0.0)
        .css_classes(["dim-label"])
        .build();
    tip.add(&label);
    page.add(&tip);
    page
}

fn modifiers(state: gdk::ModifierType) -> Mods {
    Mods {
        shift: state.contains(gdk::ModifierType::SHIFT_MASK),
        ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
        alt: state.contains(gdk::ModifierType::ALT_MASK),
        meta: state.intersects(gdk::ModifierType::SUPER_MASK | gdk::ModifierType::META_MASK),
    }
}

fn open_capture(
    shell: &Shell,
    lang: Lang,
    command: Command,
    parent: Option<gtk::Widget>,
    on_saved: impl Fn() + 'static,
) {
    let dialog = adw::AlertDialog::new(
        Some(tr(lang, Msg::PressShortcut)),
        Some(tr(lang, Msg::PressShortcutHint)),
    );
    dialog.add_response("cancel", tr(lang, Msg::Cancel));
    dialog.set_close_response("cancel");

    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let (d, s) = (dialog.downgrade(), shell.clone());
    keys.connect_key_pressed(move |_, keyval, keycode, state| {
        let Some(dialog) = d.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let mods = modifiers(state);
        if keyval == gdk::Key::Escape && mods == Mods::default() {
            return glib::Propagation::Proceed;
        }
        let result = capture(keycode, mods)
            .map_err(|e| match e {
                CaptureError::ModifierOnly => Msg::ErrModifierOnly,
                CaptureError::Unsupported => Msg::ErrUnsupportedKey,
            })
            .and_then(|h| {
                let other = command.other().get(&fresh_config()).to_string();
                if conflicts(&h, &other) {
                    Err(Msg::ErrDuplicate)
                } else {
                    Ok(h)
                }
            });
        match result {
            Err(m) => dialog.set_body(tr(lang, m)),
            Ok(h) => {
                let name = h.name();
                match update_config(|c| *command.field(c) = name) {
                    Ok(()) => on_saved(),
                    Err(e) => {
                        log::warn!("cannot save the hotkey: {e:#}");
                        s.toast(tr(lang, Msg::SaveFailed));
                    }
                }
                dialog.close();
            }
        }
        glib::Propagation::Stop
    });
    dialog.add_controller(keys);
    dialog.present(parent.as_ref());
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIFT: Mods = Mods {
        shift: true,
        ctrl: false,
        alt: false,
        meta: false,
    };

    #[test]
    fn capture_maps_keycode() {
        assert_eq!(capture(127, SHIFT).unwrap().name(), "Shift+Pause");
        assert_eq!(capture(127, Mods::default()).unwrap().name(), "Pause");
        let all = Mods {
            shift: true,
            ctrl: true,
            alt: true,
            meta: true,
        };
        assert_eq!(
            capture(65, all).unwrap().name(),
            "Ctrl+Alt+Shift+Meta+Space"
        );
        // Left Shift (50) and Left Ctrl (37) alone.
        assert_eq!(capture(50, SHIFT), Err(CaptureError::ModifierOnly));
        assert_eq!(
            capture(37, Mods::default()),
            Err(CaptureError::ModifierOnly)
        );
        // Menu (135).
        assert_eq!(
            capture(135, Mods::default()),
            Err(CaptureError::Unsupported)
        );
        // A keycode below 8 cannot be a key.
        assert_eq!(capture(3, Mods::default()), Err(CaptureError::Unsupported));
    }

    #[test]
    fn rejects_duplicate_binding() {
        let h = capture(127, SHIFT).unwrap();
        assert!(conflicts(&h, "Shift+Pause"));
        assert!(conflicts(&h, "shift + pause"));
        assert!(!conflicts(&h, "Pause"));
        assert!(!conflicts(&h, "garbage"));
    }

    #[test]
    fn reset_refuses_conflict() {
        // Selection is "Pause", Last's default is "Pause": refused.
        assert!(reset_conflicts("Pause", "Pause"));
        // Normal reset: Last is F12 now, Selection is the default Shift+Pause.
        assert!(!reset_conflicts("Pause", "Shift+Pause"));
        assert!(!reset_conflicts("Shift+Pause", "F12"));
    }
}
