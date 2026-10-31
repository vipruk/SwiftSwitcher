//! Turns raw key events into "a word was finished" events.
//!
//! Conservative by design: whenever the caret may have moved, all buffers
//! are reset, because a correction applied to the wrong place corrupts text.

use crate::hotkey::{Hotkey, HotkeyAction};
use crate::keymap::{KEY_BACKSPACE, KEY_SPACE, KeyCode, Layout, Stroke, char_for};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum Input {
    /// `value`: 0 up, 1 down, 2 repeat.
    Key {
        code: KeyCode,
        value: i32,
    },
    PointerButton,
    Touch,
}

#[derive(Debug, PartialEq)]
pub enum TrackerEvent {
    WordEnded {
        word: Vec<Stroke>,
        terminator: Stroke,
    },
    Hotkey(HotkeyAction),
}

pub struct Tracker {
    hotkeys: Vec<(Hotkey, HotkeyAction)>,
    idle: Duration,
    current: Vec<Stroke>,
    last: Option<(Vec<Stroke>, Stroke)>,
    shift: bool,
    ctrl: bool,
    alt: bool,
    meta: bool,
    caps: bool,
    /// Bit i: physical modifier key `MOD_KEYS[i]` is down.
    mod_keys: u8,
    /// Two or more modifier keys have been down together and no other key
    /// was pressed since: a modifier-only chord (e.g. `grp:alt_shift_toggle`)
    /// that may switch the layout when it is released.
    mod_chord: bool,
    last_input: Option<Instant>,
}

const SHIFTS: [KeyCode; 2] = [42, 54];
const CTRLS: [KeyCode; 2] = [29, 97];
const ALTS: [KeyCode; 2] = [56, 100];
const METAS: [KeyCode; 2] = [125, 126];
const KEY_CAPSLOCK: KeyCode = 58;
const MOD_KEYS: [KeyCode; 8] = [42, 54, 29, 97, 56, 100, 125, 126];

impl Tracker {
    pub fn new(hotkeys: Vec<(Hotkey, HotkeyAction)>, idle: Duration) -> Tracker {
        Tracker {
            hotkeys,
            idle,
            current: Vec::new(),
            last: None,
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
            caps: false,
            mod_keys: 0,
            mod_chord: false,
            last_input: None,
        }
    }

    /// Forgets the current and the last word (the caret may have moved).
    /// Caps Lock and modifier state are kept: they mirror the keyboard.
    pub fn reset(&mut self) {
        self.current.clear();
        self.last = None;
    }

    /// Returns true if `code` is a modifier and updates its state.
    fn track_modifier(&mut self, code: KeyCode, down: bool) -> bool {
        if let Some(i) = MOD_KEYS.iter().position(|&m| m == code) {
            if down {
                self.mod_keys |= 1 << i;
                if self.mod_keys.count_ones() >= 2 {
                    self.mod_chord = true;
                }
            } else {
                self.mod_keys &= !(1 << i);
                if self.mod_chord {
                    // A modifier-only chord ended: it may have switched the
                    // layout, so the buffered strokes may no longer match.
                    self.mod_chord = false;
                    self.reset();
                }
            }
        }
        if SHIFTS.contains(&code) {
            self.shift = down;
        } else if CTRLS.contains(&code) {
            self.ctrl = down;
        } else if ALTS.contains(&code) {
            self.alt = down;
        } else if METAS.contains(&code) {
            self.meta = down;
        } else {
            return false;
        }
        true
    }

    fn matching_hotkey(&self, code: KeyCode) -> Option<HotkeyAction> {
        self.hotkeys
            .iter()
            .find(|(h, _)| {
                h.code == code
                    && h.shift == self.shift
                    && h.ctrl == self.ctrl
                    && h.alt == self.alt
                    && h.meta == self.meta
            })
            .map(|(_, a)| *a)
    }

    pub fn feed(&mut self, input: Input, now: Instant) -> Option<TrackerEvent> {
        if let Some(prev) = self.last_input
            && now.saturating_duration_since(prev) > self.idle
        {
            self.reset();
        }
        self.last_input = Some(now);

        let (code, value) = match input {
            Input::PointerButton | Input::Touch => {
                self.reset();
                return None;
            }
            Input::Key { code, value } => (code, value),
        };
        if value == 0 {
            self.track_modifier(code, false);
            return None;
        }
        if self.track_modifier(code, true) {
            return None;
        }
        // Any other key pressed during a modifier chord makes it a shortcut.
        self.mod_chord = false;
        if let Some(action) = self.matching_hotkey(code) {
            return (value == 1).then_some(TrackerEvent::Hotkey(action));
        }
        if value == 2 {
            // Autorepeat is client-side on Wayland (GNOME's delay and rate),
            // so kernel repeats do not match what reached the screen.
            self.reset();
            return None;
        }
        if self.ctrl || self.alt || self.meta {
            self.reset();
            return None;
        }
        match code {
            KEY_CAPSLOCK => {
                // grp:caps_toggle switches the layout with Caps Lock.
                self.caps = !self.caps;
                self.reset();
            }
            KEY_SPACE => {
                let terminator = Stroke {
                    code,
                    shift: self.shift,
                    caps: self.caps,
                };
                if self.current.is_empty() {
                    self.last = None;
                } else {
                    let word = std::mem::take(&mut self.current);
                    self.last = Some((word.clone(), terminator));
                    return Some(TrackerEvent::WordEnded { word, terminator });
                }
            }
            KEY_BACKSPACE => {
                if self.current.pop().is_none() {
                    self.last = None;
                }
            }
            _ => {
                let stroke = Stroke {
                    code,
                    shift: self.shift,
                    caps: self.caps,
                };
                if char_for(
                    Stroke {
                        code,
                        shift: false,
                        caps: false,
                    },
                    Layout::En,
                )
                .is_some()
                {
                    self.current.push(stroke);
                } else {
                    // Enter, Tab, Esc, navigation, F-keys, media keys, ...
                    self.reset();
                }
            }
        }
        None
    }

    /// Replaces the hotkeys and idle timeout; words and key state are kept.
    pub fn reconfigure(&mut self, hotkeys: Vec<(Hotkey, HotkeyAction)>, idle: Duration) {
        self.hotkeys = hotkeys;
        self.idle = idle;
    }

    /// Seeds the Caps Lock state (e.g. from the keyboard LED at startup).
    pub fn set_caps(&mut self, on: bool) {
        self.caps = on;
    }

    pub fn current_word(&self) -> &[Stroke] {
        &self.current
    }

    /// Finished word and its terminator, while still adjacent to the caret.
    pub fn last_word(&self) -> Option<(&[Stroke], Stroke)> {
        self.last.as_ref().map(|(w, t)| (w.as_slice(), *t))
    }

    pub fn modifiers_held(&self) -> bool {
        self.shift || self.ctrl || self.alt || self.meta
    }

    /// Call after a correction or undo.
    pub fn replace_last_word(&mut self, word: Vec<Stroke>, terminator: Stroke) {
        self.last = Some((word, terminator));
        self.current.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{KEY_BACKSPACE, KEY_SPACE, Layout, stroke_for};

    const SECS5: Duration = Duration::from_secs(5);

    fn tracker() -> Tracker {
        Tracker::new(
            vec![
                (Hotkey::parse("Pause").unwrap(), HotkeyAction::ConvertLast),
                (
                    Hotkey::parse("Shift+Pause").unwrap(),
                    HotkeyAction::ConvertSelection,
                ),
            ],
            SECS5,
        )
    }

    fn key(t: &mut Tracker, code: KeyCode, value: i32, now: Instant) -> Option<TrackerEvent> {
        t.feed(Input::Key { code, value }, now)
    }

    fn tap(t: &mut Tracker, code: KeyCode, now: Instant) -> Option<TrackerEvent> {
        let e = key(t, code, 1, now);
        key(t, code, 0, now);
        e
    }

    fn feed_text(t: &mut Tracker, s: &str, now: Instant) -> Option<TrackerEvent> {
        let mut last = None;
        for c in s.chars() {
            let st = stroke_for(c, Layout::En).unwrap();
            last = tap(t, st.code, now);
        }
        last
    }

    fn filled() -> (Tracker, Instant) {
        let mut t = tracker();
        let now = Instant::now();
        feed_text(&mut t, "ghbdtn ", now);
        feed_text(&mut t, "abc", now);
        assert!(t.last_word().is_some());
        assert_eq!(t.current_word().len(), 3);
        (t, now)
    }

    fn assert_reset(t: &Tracker) {
        assert!(t.current_word().is_empty());
        assert!(t.last_word().is_none());
    }

    #[test]
    fn word_ended_on_space() {
        let mut t = tracker();
        let now = Instant::now();
        let e = feed_text(&mut t, "ghbdtn ", now).unwrap();
        match e {
            TrackerEvent::WordEnded { word, terminator } => {
                assert_eq!(word.len(), 6);
                assert_eq!(terminator.code, KEY_SPACE);
            }
            _ => panic!(),
        }
        assert!(t.last_word().is_some());
        assert!(t.current_word().is_empty());
    }

    #[test]
    fn shift_space_still_ends_word() {
        let mut t = tracker();
        let now = Instant::now();
        feed_text(&mut t, "abc", now);
        key(&mut t, 42, 1, now);
        let e = key(&mut t, KEY_SPACE, 1, now);
        assert!(matches!(e, Some(TrackerEvent::WordEnded { .. })));
    }

    #[test]
    fn backspace_pops() {
        let mut t = tracker();
        let now = Instant::now();
        feed_text(&mut t, "ghbdtnn", now);
        tap(&mut t, KEY_BACKSPACE, now);
        match feed_text(&mut t, " ", now).unwrap() {
            TrackerEvent::WordEnded { word, .. } => assert_eq!(word.len(), 6),
            _ => panic!(),
        }
    }

    #[test]
    fn held_backspace_resets() {
        // One repeat may delete any number of characters on screen.
        let mut t = tracker();
        let now = Instant::now();
        feed_text(&mut t, "x ghbdtnn", now);
        key(&mut t, KEY_BACKSPACE, 1, now);
        key(&mut t, KEY_BACKSPACE, 2, now);
        assert_reset(&t);
    }

    #[test]
    fn held_letter_resets() {
        let (mut t, now) = filled();
        key(&mut t, 30, 1, now);
        key(&mut t, 30, 2, now);
        assert_reset(&t);
        key(&mut t, 30, 0, now);
        assert_eq!(feed_text(&mut t, "b", now), None);
        assert_eq!(t.current_word().len(), 1);
    }

    #[test]
    fn modifier_repeat_does_not_reset() {
        let (mut t, now) = filled();
        key(&mut t, 42, 1, now);
        key(&mut t, 42, 2, now);
        key(&mut t, 42, 0, now);
        assert!(t.last_word().is_some());
        assert_eq!(t.current_word().len(), 3);
    }

    #[test]
    fn caps_lock_resets_and_toggles() {
        // grp:caps_toggle switches the layout with Caps Lock.
        let (mut t, now) = filled();
        tap(&mut t, KEY_CAPSLOCK, now);
        assert_reset(&t);
        feed_text(&mut t, "a", now);
        assert!(t.current_word()[0].caps);
    }

    #[test]
    fn modifier_only_chord_resets_on_release() {
        // grp:alt_shift_toggle, grp:ctrl_shift_toggle, grp:shifts_toggle.
        for (a, b) in [(56, 42), (42, 56), (29, 42), (42, 54), (97, 54)] {
            let (mut t, now) = filled();
            key(&mut t, a, 1, now);
            key(&mut t, b, 1, now);
            assert_eq!(t.current_word().len(), 3, "nothing yet while held");
            key(&mut t, b, 0, now);
            assert_reset(&t);
            // Releasing the other modifier does not reset a second time.
            key(&mut t, a, 0, now);
            assert_reset(&t);
        }
    }

    #[test]
    fn single_modifier_tap_does_not_reset() {
        for m in [42, 54, 29, 56, 125] {
            let (mut t, now) = filled();
            tap(&mut t, m, now);
            assert!(t.last_word().is_some());
            assert_eq!(t.current_word().len(), 3);
        }
    }

    #[test]
    fn shift_typing_is_not_a_chord() {
        let mut t = tracker();
        let now = Instant::now();
        key(&mut t, 42, 1, now);
        feed_text(&mut t, "ab", now);
        key(&mut t, 42, 0, now);
        assert_eq!(t.current_word().len(), 2);
    }

    #[test]
    fn held_backspace_clears_last() {
        let mut t = tracker();
        let now = Instant::now();
        feed_text(&mut t, "x abc", now);
        assert!(t.last_word().is_some());
        key(&mut t, KEY_BACKSPACE, 1, now);
        for _ in 0..10 {
            key(&mut t, KEY_BACKSPACE, 2, now);
        }
        assert_reset(&t);
    }

    #[test]
    fn pointer_touch_enter_reset() {
        let (mut t, now) = filled();
        t.feed(Input::PointerButton, now);
        assert_reset(&t);
        let (mut t, now) = filled();
        t.feed(Input::Touch, now);
        assert_reset(&t);
        let (mut t, now) = filled();
        key(&mut t, 28, 1, now);
        assert_reset(&t);
    }

    #[test]
    fn navigation_keys_reset() {
        for code in [15, 1, 103, 105, 106, 108, 102, 107, 104, 109, 111, 96] {
            let (mut t, now) = filled();
            key(&mut t, code, 1, now);
            assert_reset(&t);
        }
    }

    #[test]
    fn unmapped_keys_reset() {
        for code in [64, 87, 110, 113, 163] {
            let (mut t, now) = filled();
            key(&mut t, code, 1, now);
            assert_reset(&t);
        }
        let (mut t, now) = filled();
        key(&mut t, 64, 2, now); // F6 repeat
        assert_reset(&t);
    }

    #[test]
    fn ctrl_combo_resets_modifier_alone_does_not() {
        let (mut t, now) = filled();
        key(&mut t, 29, 1, now);
        assert!(t.last_word().is_some());
        assert_eq!(t.current_word().len(), 3);
        key(&mut t, 30, 1, now);
        assert_reset(&t);
        for m in [56, 125] {
            let (mut t, now) = filled();
            key(&mut t, m, 1, now);
            key(&mut t, 30, 1, now);
            assert_reset(&t);
        }
    }

    #[test]
    fn idle_gap_resets_before_processing() {
        let (mut t, now) = filled();
        let later = now + Duration::from_secs(6);
        key(&mut t, 30, 1, later);
        assert!(t.last_word().is_none());
        assert_eq!(t.current_word().len(), 1);
        let (mut t, now) = filled();
        key(&mut t, 30, 0, now + Duration::from_secs(5));
        assert!(t.last_word().is_some());
        let (mut t, now) = filled();
        key(&mut t, 30, 0, now + Duration::from_secs(6));
        assert_reset(&t);
    }

    #[test]
    fn keyup_never_changes_buffers() {
        let (mut t, now) = filled();
        for code in [28, 103, 64, 14, 57] {
            key(&mut t, code, 0, now);
        }
        assert!(t.last_word().is_some());
        assert_eq!(t.current_word().len(), 3);
    }

    #[test]
    fn hotkeys() {
        let (mut t, now) = filled();
        assert_eq!(
            key(&mut t, 119, 1, now),
            Some(TrackerEvent::Hotkey(HotkeyAction::ConvertLast))
        );
        assert!(t.last_word().is_some());
        assert_eq!(t.current_word().len(), 3);
        // a hotkey's repeat is ignored and does not reset
        assert_eq!(key(&mut t, 119, 2, now), None);
        assert_eq!(t.current_word().len(), 3);
        key(&mut t, 119, 0, now);
        key(&mut t, 42, 1, now);
        assert_eq!(
            key(&mut t, 119, 1, now),
            Some(TrackerEvent::Hotkey(HotkeyAction::ConvertSelection))
        );
        assert!(t.last_word().is_some());
        assert_eq!(t.current_word().len(), 3);
    }

    #[test]
    fn pause_with_shift_does_not_match_convert_last() {
        let mut t = Tracker::new(
            vec![(Hotkey::parse("Pause").unwrap(), HotkeyAction::ConvertLast)],
            SECS5,
        );
        let now = Instant::now();
        feed_text(&mut t, "abc", now);
        key(&mut t, 42, 1, now);
        assert_eq!(key(&mut t, 119, 1, now), None);
        assert!(t.current_word().is_empty()); // unmapped key reset
    }

    #[test]
    fn hotkey_with_ctrl_matches_despite_ctrl_reset_rule() {
        let mut t = Tracker::new(
            vec![(
                Hotkey::parse("Ctrl+Alt+Space").unwrap(),
                HotkeyAction::ConvertLast,
            )],
            SECS5,
        );
        let now = Instant::now();
        feed_text(&mut t, "abc", now);
        key(&mut t, 29, 1, now);
        key(&mut t, 56, 1, now);
        assert!(key(&mut t, KEY_SPACE, 1, now).is_some());
        assert_eq!(t.current_word().len(), 3);
    }

    #[test]
    fn ctrl_alt_space_hotkey_keeps_last_word() {
        let mut t = Tracker::new(
            vec![(
                Hotkey::parse("Ctrl+Alt+Space").unwrap(),
                HotkeyAction::ConvertLast,
            )],
            SECS5,
        );
        let now = Instant::now();
        feed_text(&mut t, "ghbdtn ", now);
        key(&mut t, 29, 1, now);
        key(&mut t, 56, 1, now);
        assert_eq!(
            key(&mut t, KEY_SPACE, 1, now),
            Some(TrackerEvent::Hotkey(HotkeyAction::ConvertLast))
        );
        key(&mut t, KEY_SPACE, 0, now);
        key(&mut t, 56, 0, now);
        key(&mut t, 29, 0, now);
        let (word, _) = t.last_word().expect("last word kept");
        let typed: Vec<_> = "ghbdtn"
            .chars()
            .map(|c| stroke_for(c, Layout::En).unwrap())
            .collect();
        assert_eq!(word, typed.as_slice());
    }

    #[test]
    fn modifiers_held_tracks_state() {
        let mut t = tracker();
        let now = Instant::now();
        assert!(!t.modifiers_held());
        key(&mut t, 42, 1, now);
        assert!(t.modifiers_held());
        key(&mut t, 42, 0, now);
        assert!(!t.modifiers_held());
        key(&mut t, 97, 1, now);
        assert!(t.modifiers_held());
    }

    #[test]
    fn set_caps_applies_to_typed_strokes() {
        let mut t = tracker();
        let now = Instant::now();
        t.set_caps(true);
        feed_text(&mut t, "a", now);
        assert!(t.current_word()[0].caps);
    }

    #[test]
    fn caps_lock_toggles_and_shift_recorded() {
        let mut t = tracker();
        let now = Instant::now();
        tap(&mut t, 58, now);
        feed_text(&mut t, "a", now);
        assert!(t.current_word()[0].caps);
        assert!(!t.current_word()[0].shift);
        tap(&mut t, 58, now); // also resets the buffer (finding 3)
        key(&mut t, 54, 1, now);
        tap(&mut t, 30, now);
        assert!(!t.current_word()[0].caps);
        assert!(t.current_word()[0].shift);
    }

    #[test]
    fn space_on_empty_word_clears_last() {
        let mut t = tracker();
        let now = Instant::now();
        feed_text(&mut t, "abc ", now);
        assert!(t.last_word().is_some());
        assert_eq!(tap(&mut t, KEY_SPACE, now), None);
        assert!(t.last_word().is_none());
    }

    #[test]
    fn replace_last_word_sets_and_clears_current() {
        let mut t = tracker();
        let now = Instant::now();
        feed_text(&mut t, "abc", now);
        let w = vec![stroke_for('x', Layout::En).unwrap()];
        let term = stroke_for(' ', Layout::En).unwrap();
        t.replace_last_word(w.clone(), term);
        assert!(t.current_word().is_empty());
        let (lw, lt) = t.last_word().unwrap();
        assert_eq!(lw, &w[..]);
        assert_eq!(lt, term);
    }
}
