//! Correction orchestration: decides when to erase and retype the user's text.
//!
//! The overriding rule is to never corrupt text. Whenever anything is uncertain
//! (the layout did not switch, the user kept typing, modifiers are held, a D-Bus
//! call failed) the action is abandoned before anything is injected: the layout
//! is switched and confirmed first, and only then are keys sent.
//! Log lines carry only lengths, verdict reasons, layout ids and app ids.

use crate::config::Config;
use crate::desktop::Desktop;
use crate::inject::Injector;
use crate::input::Physical;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU64, Ordering};
use std::time::Duration;
use swift_engine::detect::{Params, Verdict, decide};
use swift_engine::dict::UserDict;
use swift_engine::hotkey::{Hotkey, HotkeyAction};
use swift_engine::keymap::{KeyCode, Layout, Stroke, char_for};
use swift_engine::tracker::{Input, Tracker, TrackerEvent};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::time::{sleep, timeout};

const LAYOUT_POLL: Duration = Duration::from_millis(5);
const LAYOUT_TIMEOUT: Duration = Duration::from_millis(300);
const MODS_POLL: Duration = Duration::from_millis(10);
const MODS_TIMEOUT: Duration = Duration::from_secs(1);
/// Mutter drops injected key events while physical keys are held, so every
/// injection waits until no key is down (R19).
const KEYS_POLL: Duration = Duration::from_millis(5);
const KEYS_TIMEOUT: Duration = Duration::from_millis(300);
const COPY_WAIT: Duration = Duration::from_millis(100);
const PASTE_WAIT: Duration = Duration::from_millis(200);

const KEY_LEFTCTRL: KeyCode = 29;
const KEY_LEFTSHIFT: KeyCode = 42;
const KEY_C: KeyCode = 46;
const KEY_V: KeyCode = 47;

/// Ok iff the input sources are exactly `us` and `ru`, one each.
pub fn layout_status(layouts: &[String]) -> Result<(), String> {
    let count = |id: &str| layouts.iter().filter(|l| *l == id).count();
    if layouts.len() == 2 && count("us") == 1 && count("ru") == 1 {
        Ok(())
    } else {
        Err(format!(
            "input sources are {layouts:?}; exactly \"us\" and \"ru\" are required"
        ))
    }
}

fn source_id(l: Layout) -> &'static str {
    match l {
        Layout::En => "us",
        Layout::Ru => "ru",
    }
}

// ---- physical activity ----

/// Modifier keys in pairs (left, right): Shift, Ctrl, Alt, Meta.
const MOD_KEYS: [KeyCode; 8] = [42, 54, 29, 97, 56, 100, 125, 126];
/// Bits 0..8: each modifier key held. Bits 8..12: modifier class as the
/// Tracker sees it (set by either key's down, cleared by either key's up),
/// used for hotkey matching so it agrees with the Tracker.
const KEYS_MASK: u16 = 0xff;
const CLASS_SHIFT: u16 = 1 << 8;
const CLASS_CTRL: u16 = 1 << 9;
const CLASS_ALT: u16 = 1 << 10;
const CLASS_META: u16 = 1 << 11;

/// Applies one input to the modifier state. Returns the new state and whether
/// the input is physical activity (R1): key down/repeat of a non-modifier,
/// non-hotkey key, or a pointer button / touch.
fn step(state: u16, input: &Input, hotkeys: &[(Hotkey, HotkeyAction)]) -> (u16, bool) {
    let (code, value) = match *input {
        Input::PointerButton | Input::Touch => return (state, true),
        Input::Key { code, value } => (code, value),
    };
    if let Some(i) = MOD_KEYS.iter().position(|&m| m == code) {
        let bits = (1u16 << i) | (1u16 << (8 + i / 2));
        let state = if value == 0 {
            state & !bits
        } else {
            state | bits
        };
        return (state, false);
    }
    if value == 0 {
        return (state, false);
    }
    let held = |bit: u16| state & bit != 0;
    let hotkey = hotkeys.iter().any(|(h, _)| {
        h.code == code
            && h.shift == held(CLASS_SHIFT)
            && h.ctrl == held(CLASS_CTRL)
            && h.alt == held(CLASS_ALT)
            && h.meta == held(CLASS_META)
    });
    (state, !hotkey)
}

/// Physical activity seen in real time, ahead of the Corrector's queue.
///
/// The Corrector handles events one at a time, so while it waits (layout
/// settle, modifier release) it cannot see queued events. The daemon therefore
/// calls [`Activity::observe`] for every event, in order, *before* queueing it
/// for the Corrector, stamped with the resulting `seq` (see [`forward`] and
/// [`Stamped`]). The Corrector compares the live `seq` with the stamp of the
/// last event it handled: a difference means the user did something it has
/// not processed yet, and it must not inject.
///
/// Activity is the only place that classifies inputs (R1, modifiers,
/// hotkeys). Single writer: only one task may call `observe`.
///
/// It also tracks every physical key that is down (any code up to
/// `KEY_MAX`, modifiers included; R19) and whether a pointer device is being
/// read (set by the input reader).
#[derive(Default)]
pub struct Activity {
    seq: AtomicU64,
    mods: AtomicU16,
    held: [AtomicU64; HELD_WORDS],
    pointer: AtomicBool,
}

/// Bit words covering key codes `0..=KEY_MAX` (0x2ff).
const HELD_WORDS: usize = 12;

impl Activity {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one input and returns `seq` as of right after it.
    pub fn observe(&self, input: &Input, hotkeys: &[(Hotkey, HotkeyAction)]) -> u64 {
        let (state, counted) = step(self.mods.load(Ordering::SeqCst), input, hotkeys);
        self.mods.store(state, Ordering::SeqCst);
        if let Input::Key { code, value } = *input
            && let Some(word) = self.held.get(usize::from(code) / 64)
        {
            let bit = 1u64 << (code % 64);
            if value == 0 {
                word.fetch_and(!bit, Ordering::SeqCst);
            } else {
                word.fetch_or(bit, Ordering::SeqCst);
            }
        }
        if counted {
            self.seq.fetch_add(1, Ordering::SeqCst) + 1
        } else {
            self.seq()
        }
    }

    /// Number of activity events observed so far (R1).
    pub fn seq(&self) -> u64 {
        self.seq.load(Ordering::SeqCst)
    }

    /// Whether any physical modifier key is down right now.
    pub fn modifiers_held(&self) -> bool {
        self.mods.load(Ordering::SeqCst) & KEYS_MASK != 0
    }

    /// Whether any physical key (modifiers included) is down right now.
    pub fn keys_held(&self) -> bool {
        self.held.iter().any(|w| w.load(Ordering::SeqCst) != 0)
    }

    /// Set by the input reader: whether a mouse/touchpad/touchscreen is read.
    pub fn set_pointer_present(&self, present: bool) {
        self.pointer.store(present, Ordering::SeqCst);
    }

    pub fn pointer_present(&self) -> bool {
        self.pointer.load(Ordering::SeqCst)
    }
}

/// A physical event and the `Activity::seq()` right after it was observed.
pub struct Stamped {
    pub physical: Physical,
    pub seq: u64,
}

/// Moves events from the reader to the Corrector, observing and stamping
/// each one first.
pub async fn forward(
    mut rx: Receiver<Physical>,
    tx: Sender<Stamped>,
    activity: Arc<Activity>,
    hotkeys: Vec<(Hotkey, HotkeyAction)>,
) {
    while let Some(physical) = rx.recv().await {
        let seq = activity.observe(&physical.input, &hotkeys);
        if tx.send(Stamped { physical, seq }).await.is_err() {
            break;
        }
    }
}

// ---- corrector ----

/// How a [`Corrector::correct`] attempt ended.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Outcome {
    /// Nothing was injected; the layout was put back.
    Abandoned,
    /// Injected with no physical input in the meantime.
    Done,
    /// Injected, but the user typed during the replay: where those keys
    /// landed is unknown, so the tracker was reset.
    Disturbed,
}

/// The last automatic correction, while it is still undoable.
struct Correction {
    strokes: Vec<Stroke>,
    terminator: Stroke,
    from: Layout,
}

pub struct Corrector<D: Desktop, I: Injector> {
    desktop: D,
    injector: I,
    tracker: Tracker,
    dict: UserDict,
    config: Config,
    data_dir: PathBuf,
    params: Params,
    activity: Arc<Activity>,
    /// Stamp of the last handled event; equals `activity.seq()` when idle.
    last_stamp: u64,
    last_correction: Option<Correction>,
    /// Events stamped up to this seq were typed during a replay; they only
    /// update modifier and Caps Lock state, never the word buffers.
    disturbed_until: Option<u64>,
    /// The current word was converted by hand (Pause mid-word): it must not
    /// be auto-corrected when it ends.
    user_decided: bool,
}

impl<D: Desktop, I: Injector> Corrector<D, I> {
    pub fn new(
        desktop: D,
        injector: I,
        config: Config,
        dict: UserDict,
        data_dir: PathBuf,
        activity: Arc<Activity>,
        initial_caps: Option<bool>,
    ) -> Result<Self> {
        let hotkeys = config.hotkeys()?;
        let mut tracker = Tracker::new(hotkeys, Duration::from_secs(config.idle_reset_secs));
        if let Some(caps) = initial_caps {
            tracker.set_caps(caps);
        }
        Ok(Self {
            desktop,
            injector,
            tracker,
            dict,
            config,
            data_dir,
            params: Params::default(),
            last_stamp: activity.seq(),
            activity,
            last_correction: None,
            disturbed_until: None,
            user_decided: false,
        })
    }

    /// Feeds one event stamped by [`forward`] and acts on it.
    pub async fn handle(&mut self, ev: Stamped) -> Result<()> {
        let Stamped { physical: p, seq } = ev;
        if seq != self.last_stamp {
            // Physical activity other than a hotkey: nothing is undoable now.
            self.last_stamp = seq;
            self.last_correction = None;
        }
        let event = self.tracker.feed(p.input, p.at);
        if self.disturbed_until.is_some_and(|d| seq <= d) {
            // Typed while a replay ran: where it landed is unknown.
            self.tracker.reset();
            self.user_decided = false;
            return Ok(());
        }
        self.disturbed_until = None;
        let decided = self.user_decided;
        if self.tracker.current_word().is_empty() {
            self.user_decided = false;
        }
        match event {
            Some(TrackerEvent::WordEnded { .. }) if decided => {
                log::debug!("word was converted by hand; not auto-correcting it");
                Ok(())
            }
            Some(TrackerEvent::WordEnded { word, terminator }) => {
                self.on_word(word, terminator).await
            }
            Some(TrackerEvent::Hotkey(HotkeyAction::ConvertLast)) => self.convert_last().await,
            Some(TrackerEvent::Hotkey(HotkeyAction::ConvertSelection)) => {
                self.convert_selection().await
            }
            None => Ok(()),
        }
    }

    /// No physical activity is waiting that this Corrector has not handled.
    fn quiet(&self) -> bool {
        self.activity.seq() == self.last_stamp
    }

    async fn wait_modifiers_released(&self) -> bool {
        let wait = async {
            while self.activity.modifiers_held() {
                sleep(MODS_POLL).await;
            }
        };
        let ok = timeout(MODS_TIMEOUT, wait).await.is_ok();
        if !ok {
            log::debug!("modifiers still held; action abandoned");
        }
        ok
    }

    /// Waits until no physical key is down, up to `KEYS_TIMEOUT` (R19).
    async fn wait_keys_released(&self) -> bool {
        let wait = async {
            while self.activity.keys_held() {
                sleep(KEYS_POLL).await;
            }
        };
        let ok = timeout(KEYS_TIMEOUT, wait).await.is_ok();
        if !ok {
            log::debug!("keys still held; action abandoned");
        }
        ok
    }

    /// Current layout, if the input sources are supported and it is one of them.
    async fn supported_layout(&self) -> Option<Layout> {
        let layouts = match self.desktop.layouts().await {
            Ok(l) => l,
            Err(e) => {
                log::warn!("cannot list input sources: {e:#}");
                return None;
            }
        };
        if let Err(reason) = layout_status(&layouts) {
            log::debug!("not acting: {reason}");
            return None;
        }
        match self.desktop.layout().await {
            Ok(id) => {
                let l = Layout::from_source_id(&id);
                if l.is_none() {
                    log::debug!("not acting: current input source {id:?}");
                }
                l
            }
            Err(e) => {
                log::warn!("cannot read input source: {e:#}");
                None
            }
        }
    }

    async fn focused_app(&self) -> Option<String> {
        match self.desktop.focused_app().await {
            Ok(a) => Some(a),
            Err(e) => {
                log::warn!("cannot read focused app: {e:#}");
                None
            }
        }
    }

    async fn on_word(&mut self, word: Vec<Stroke>, terminator: Stroke) -> Result<()> {
        if !self.config.auto_correct || !self.quiet() {
            return Ok(());
        }
        if !self.activity.pointer_present() {
            // Clicks could move the caret unseen; see input::spawn_reader.
            log::debug!("no pointer device read; automatic correction is off");
            return Ok(());
        }
        let Some(from) = self.supported_layout().await else {
            return Ok(());
        };
        let Some(app) = self.focused_app().await else {
            return Ok(());
        };
        if self.config.is_excluded(&app) {
            log::debug!("not acting in excluded app {app}");
            return Ok(());
        }
        match decide(&word, from, &self.dict, &self.params) {
            Verdict::Keep(reason) => {
                log::debug!("kept word of {} strokes: {reason:?}", word.len());
                Ok(())
            }
            Verdict::Convert => {
                let erase = word.len() + 1;
                let outcome = self
                    .correct(&word, Some(terminator), erase, from, from.other())
                    .await?;
                if outcome == Outcome::Done {
                    self.last_correction = Some(Correction {
                        strokes: word,
                        terminator,
                        from,
                    });
                }
                Ok(())
            }
        }
    }

    /// Switches to `to`, confirms it, waits `settle_ms` and for all keys to be
    /// released, then erases `erase` characters and retypes `strokes` (and
    /// `terminator`). When abandoned nothing was injected and the layout was
    /// put back to `from`.
    async fn correct(
        &mut self,
        strokes: &[Stroke],
        terminator: Option<Stroke>,
        erase: usize,
        from: Layout,
        to: Layout,
    ) -> Result<Outcome> {
        if !self.quiet() || !self.wait_modifiers_released().await {
            return Ok(Outcome::Abandoned);
        }
        let to_id = source_id(to);
        let switched = match self.desktop.set_layout(to_id).await {
            Ok(true) => self.wait_layout(to_id).await,
            Ok(false) => {
                log::warn!("input source {to_id} was refused");
                false
            }
            Err(e) => {
                log::warn!("cannot switch input source to {to_id}: {e:#}");
                false
            }
        };
        if !switched {
            self.restore(from).await;
            return Ok(Outcome::Abandoned);
        }
        sleep(Duration::from_millis(self.config.settle_ms)).await;
        // Mutter drops injected events while any physical key (e.g. the
        // Space that ended the word) is still down.
        if !self.wait_keys_released().await || !self.quiet() || self.activity.keys_held() {
            log::debug!("input arrived during settle; correction abandoned");
            self.restore(from).await;
            return Ok(Outcome::Abandoned);
        }
        if let Err(e) = self.inject(strokes, terminator, erase) {
            // The text near the caret is now unknown; forget everything.
            self.forget();
            return Err(e);
        }
        if !self.quiet() {
            log::debug!("input arrived during the replay; forgetting the text");
            self.forget();
            self.disturbed_until = Some(self.activity.seq());
            return Ok(Outcome::Disturbed);
        }
        if let Some(t) = terminator {
            self.tracker.replace_last_word(strokes.to_vec(), t);
        }
        log::info!(
            "converted {} strokes {} -> {}",
            strokes.len(),
            source_id(from),
            to_id
        );
        Ok(Outcome::Done)
    }

    /// Forgets all text state (Caps Lock and modifiers are kept).
    fn forget(&mut self) {
        self.tracker.reset();
        self.last_correction = None;
        self.user_decided = false;
    }

    fn inject(
        &mut self,
        strokes: &[Stroke],
        terminator: Option<Stroke>,
        erase: usize,
    ) -> Result<()> {
        self.injector.backspace(erase)?;
        for &s in strokes {
            self.injector.tap(s)?;
        }
        if let Some(t) = terminator {
            self.injector.tap(t)?;
        }
        Ok(())
    }

    /// Polls the current layout until it is `id`, up to `LAYOUT_TIMEOUT`.
    async fn wait_layout(&self, id: &str) -> bool {
        let poll = async {
            loop {
                match self.desktop.layout().await {
                    Ok(cur) if cur == id => return true,
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("cannot read input source: {e:#}");
                        return false;
                    }
                }
                sleep(LAYOUT_POLL).await;
            }
        };
        let ok = timeout(LAYOUT_TIMEOUT, poll).await.unwrap_or(false);
        if !ok {
            log::warn!("input source did not become {id}; nothing changed");
        }
        ok
    }

    /// Best-effort switch back after an abandoned correction.
    async fn restore(&self, from: Layout) {
        let id = source_id(from);
        if let Err(e) = self.desktop.set_layout(id).await {
            log::warn!("cannot switch input source back to {id}: {e:#}");
        }
    }

    async fn convert_last(&mut self) -> Result<()> {
        if !self.quiet() {
            return Ok(());
        }
        if let Some(c) = self.last_correction.take() {
            // Only while the corrected word is still what the tracker last
            // saw next to the caret (e.g. not after an idle reset).
            let adjacent = self.tracker.current_word().is_empty()
                && self.tracker.last_word() == Some((c.strokes.as_slice(), c.terminator));
            if adjacent {
                return self.undo(c).await;
            }
        }
        let Some(from) = self.supported_layout().await else {
            return Ok(());
        };
        let to = from.other();
        if !self.tracker.current_word().is_empty() {
            let word = self.tracker.current_word().to_vec();
            if self.correct(&word, None, word.len(), from, to).await? == Outcome::Done {
                self.user_decided = true;
            }
        } else if let Some((word, t)) = self.tracker.last_word() {
            let word = word.to_vec();
            self.correct(&word, Some(t), word.len() + 1, from, to)
                .await?;
        }
        Ok(())
    }

    /// Restores the original text of the last correction and learns the word.
    async fn undo(&mut self, c: Correction) -> Result<()> {
        let erase = c.strokes.len() + 1;
        let undone = self
            .correct(
                &c.strokes,
                Some(c.terminator),
                erase,
                c.from.other(),
                c.from,
            )
            .await?;
        if undone == Outcome::Abandoned {
            return Ok(());
        }
        let typed: Option<String> = c.strokes.iter().map(|&s| char_for(s, c.from)).collect();
        if let Some(typed) = typed
            && let Err(e) = self.dict.add_never(&self.data_dir, &typed.to_lowercase())
        {
            log::warn!("cannot update the never list: {e}");
        }
        Ok(())
    }

    async fn convert_selection(&mut self) -> Result<()> {
        self.last_correction = None;
        if !self.wait_modifiers_released().await || !self.quiet() {
            return Ok(());
        }
        let Some(from) = self.supported_layout().await else {
            return Ok(());
        };
        let Some(app) = self.focused_app().await else {
            return Ok(());
        };
        let mods: &[KeyCode] = if self.config.is_terminal(&app) {
            &[KEY_LEFTCTRL, KEY_LEFTSHIFT]
        } else {
            &[KEY_LEFTCTRL]
        };
        let begun = match self.desktop.selection_begin().await {
            Ok(true) => true,
            Ok(false) => {
                log::debug!("selection convert refused");
                false
            }
            Err(e) => {
                log::warn!("cannot begin selection convert: {e:#}");
                false
            }
        };
        let pasted = if begun {
            self.copy_convert_paste(mods).await
        } else {
            Ok(None)
        };
        // Always, even when Begin failed or timed out: the extension may have
        // saved the clipboard after all, and End is what restores it.
        if let Err(e) = self.desktop.selection_end().await {
            log::warn!("cannot end selection convert: {e:#}");
        }
        // Switch to the layout the converted text is now in.
        if let Some(to) = pasted? {
            match self.desktop.set_layout(source_id(to)).await {
                Ok(true) => log::info!("converted selection; input source {}", source_id(to)),
                Ok(false) => log::warn!("input source {} was refused", source_id(to)),
                Err(e) => log::warn!("cannot switch input source to {}: {e:#}", source_id(to)),
            }
        } else if begun {
            log::debug!(
                "selection not converted; input source kept ({})",
                source_id(from)
            );
        }
        Ok(())
    }

    /// Copy, convert the clipboard, paste. Returns the layout of the pasted
    /// text iff it pasted.
    async fn copy_convert_paste(&mut self, mods: &[KeyCode]) -> Result<Option<Layout>> {
        if !self.wait_keys_released().await || !self.quiet() {
            return Ok(None);
        }
        self.injector.chord(mods, KEY_C)?;
        sleep(COPY_WAIT).await;
        let target = match self.desktop.selection_convert().await {
            Ok(id) if id.is_empty() => {
                log::debug!("nothing to convert in the selection");
                return Ok(None);
            }
            Ok(id) => Layout::from_source_id(&id),
            Err(e) => {
                log::warn!("cannot convert selection: {e:#}");
                return Ok(None);
            }
        };
        let Some(target) = target else {
            log::warn!("selection converted to an unknown layout; not pasting");
            return Ok(None);
        };
        if !self.wait_keys_released().await || !self.quiet() {
            log::debug!("input arrived during selection convert; not pasting");
            return Ok(None);
        }
        self.injector.chord(mods, KEY_V)?;
        sleep(PASTE_WAIT).await;
        Ok(Some(target))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::{Duration, Instant};
    use swift_engine::keymap::{KEY_SPACE, KeyCode, Layout, Stroke, stroke_for};
    use tokio::time::sleep;

    const KEY_PAUSE: KeyCode = 119;
    const KEY_SHIFT: KeyCode = 42;

    #[derive(Clone, Debug, PartialEq)]
    enum Op {
        Tap(Stroke),
        Backspace(usize),
        Chord(Vec<KeyCode>, KeyCode),
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Ev {
        Call(String),
        Op(Op),
    }

    type Log = Rc<RefCell<Vec<Ev>>>;

    #[derive(Clone, Copy)]
    enum Switch {
        Now,
        Lag(u32),
        Never,
    }

    struct FakeDesktop {
        log: Log,
        layout: RefCell<String>,
        target: RefCell<Option<(String, u32)>>,
        layouts: Vec<String>,
        app: String,
        switch: Switch,
        /// What SelectionConvert returns: the target id, "" = not converted.
        convert_target: String,
        begin_fails: bool,
    }

    impl FakeDesktop {
        fn call(&self, s: &str) {
            self.log.borrow_mut().push(Ev::Call(s.to_string()));
        }
    }

    impl Desktop for FakeDesktop {
        async fn layout(&self) -> Result<String> {
            let mut t = self.target.borrow_mut();
            if let Some((id, n)) = t.as_mut() {
                if *n == 0 {
                    *self.layout.borrow_mut() = id.clone();
                    *t = None;
                } else {
                    *n -= 1;
                }
            }
            Ok(self.layout.borrow().clone())
        }
        async fn layouts(&self) -> Result<Vec<String>> {
            Ok(self.layouts.clone())
        }
        async fn set_layout(&self, id: &str) -> Result<bool> {
            self.call(&format!("set_layout {id}"));
            match self.switch {
                Switch::Now => *self.layout.borrow_mut() = id.to_string(),
                Switch::Lag(n) => *self.target.borrow_mut() = Some((id.to_string(), n)),
                Switch::Never => {}
            }
            Ok(true)
        }
        async fn focused_app(&self) -> Result<String> {
            Ok(self.app.clone())
        }
        async fn selection_begin(&self) -> Result<bool> {
            self.call("selection_begin");
            if self.begin_fails {
                anyhow::bail!("D-Bus call timed out");
            }
            Ok(true)
        }
        async fn selection_convert(&self) -> Result<String> {
            self.call("selection_convert");
            Ok(self.convert_target.clone())
        }
        async fn selection_end(&self) -> Result<bool> {
            self.call("selection_end");
            Ok(true)
        }
    }

    type Disturb = (Arc<Activity>, Vec<(Hotkey, HotkeyAction)>, &'static str);

    struct FakeInjector {
        log: Log,
        /// Text the user "types" during the first injected op, observed by
        /// Activity like the forwarder would; the stamped events are kept in
        /// `typed` for the test to hand to the Corrector afterwards.
        disturb: Option<Disturb>,
        typed: Vec<Stamped>,
        fail: bool,
    }

    impl FakeInjector {
        fn op(&mut self, op: Op) -> anyhow::Result<()> {
            if self.fail {
                anyhow::bail!("uinput write failed");
            }
            self.log.borrow_mut().push(Ev::Op(op));
            if let Some((a, hk, text)) = self.disturb.take() {
                for ch in text.chars() {
                    let code = stroke_for(ch, Layout::En).unwrap().code;
                    for value in [1, 0] {
                        let input = key_input(code, value);
                        let seq = a.observe(&input, &hk);
                        self.typed.push(stamped(input, seq));
                    }
                }
            }
            Ok(())
        }
    }

    impl Injector for FakeInjector {
        fn tap(&mut self, s: Stroke) -> anyhow::Result<()> {
            self.op(Op::Tap(s))
        }
        fn backspace(&mut self, n: usize) -> anyhow::Result<()> {
            self.op(Op::Backspace(n))
        }
        fn chord(&mut self, mods: &[KeyCode], key: KeyCode) -> anyhow::Result<()> {
            self.op(Op::Chord(mods.to_vec(), key))
        }
    }

    struct Rig {
        c: Corrector<FakeDesktop, FakeInjector>,
        log: Log,
        activity: Arc<Activity>,
        hotkeys: Vec<(Hotkey, HotkeyAction)>,
        dir: tempfile::TempDir,
    }

    struct Setup {
        layouts: Vec<&'static str>,
        app: &'static str,
        switch: Switch,
        convert_target: &'static str,
        begin_fails: bool,
        pointer: bool,
        config: Config,
    }

    impl Default for Setup {
        fn default() -> Self {
            Setup {
                layouts: vec!["us", "ru"],
                app: "org.gnome.TextEditor",
                switch: Switch::Now,
                convert_target: "ru",
                begin_fails: false,
                pointer: true,
                config: Config::default(),
            }
        }
    }

    fn rig(s: Setup) -> Rig {
        let log: Log = Rc::default();
        let dir = tempfile::tempdir().unwrap();
        let desktop = FakeDesktop {
            log: log.clone(),
            layout: RefCell::new("us".into()),
            target: RefCell::new(None),
            layouts: s.layouts.iter().map(|l| l.to_string()).collect(),
            app: s.app.into(),
            switch: s.switch,
            convert_target: s.convert_target.into(),
            begin_fails: s.begin_fails,
        };
        let injector = FakeInjector {
            log: log.clone(),
            disturb: None,
            typed: Vec::new(),
            fail: false,
        };
        let activity = Arc::new(Activity::new());
        activity.set_pointer_present(s.pointer);
        let hotkeys = s.config.hotkeys().unwrap();
        let dict = UserDict::load(dir.path()).unwrap();
        let c = Corrector::new(
            desktop,
            injector,
            s.config,
            dict,
            dir.path().to_path_buf(),
            activity.clone(),
            None,
        )
        .unwrap();
        Rig {
            c,
            log,
            activity,
            hotkeys,
            dir,
        }
    }

    /// A new Corrector sharing the rig's log, data dir and Activity.
    fn rebuilt(r: &Rig) -> Corrector<FakeDesktop, FakeInjector> {
        let desktop = FakeDesktop {
            log: r.log.clone(),
            layout: RefCell::new("us".into()),
            target: RefCell::new(None),
            layouts: strings(&["us", "ru"]),
            app: "org.gnome.TextEditor".into(),
            switch: Switch::Now,
            convert_target: "ru".into(),
            begin_fails: false,
        };
        let injector = FakeInjector {
            log: r.log.clone(),
            disturb: None,
            typed: Vec::new(),
            fail: false,
        };
        let dict = UserDict::load(r.dir.path()).unwrap();
        Corrector::new(
            desktop,
            injector,
            Config::default(),
            dict,
            r.dir.path().to_path_buf(),
            r.activity.clone(),
            None,
        )
        .unwrap()
    }

    fn key_input(code: KeyCode, value: i32) -> Input {
        Input::Key { code, value }
    }

    fn stamped(input: Input, seq: u64) -> Stamped {
        Stamped {
            physical: phys(input),
            seq,
        }
    }

    fn phys(input: Input) -> Physical {
        Physical {
            input,
            at: Instant::now(),
        }
    }

    impl Rig {
        /// What the daemon's forwarder does: observe, then hand to the Corrector.
        async fn feed(&mut self, input: Input) {
            let seq = self.activity.observe(&input, &self.hotkeys);
            self.c.handle(stamped(input, seq)).await.unwrap();
        }

        async fn key(&mut self, code: KeyCode, value: i32) {
            self.feed(key_input(code, value)).await;
        }

        /// A key tapped faster than the Corrector handles it: Activity has
        /// seen the release before the Corrector sees the press.
        async fn tap(&mut self, code: KeyCode) {
            self.tap_at(code, Instant::now()).await;
        }

        async fn tap_at(&mut self, code: KeyCode, at: Instant) {
            let (down, up) = (key_input(code, 1), key_input(code, 0));
            let down_seq = self.activity.observe(&down, &self.hotkeys);
            let up_seq = self.activity.observe(&up, &self.hotkeys);
            for (input, seq) in [(down, down_seq), (up, up_seq)] {
                let ev = Stamped {
                    physical: Physical { input, at },
                    seq,
                };
                self.c.handle(ev).await.unwrap();
            }
        }

        /// Hands the events the user typed during a replay to the Corrector.
        async fn deliver_typed(&mut self) {
            for ev in std::mem::take(&mut self.c.injector.typed) {
                self.c.handle(ev).await.unwrap();
            }
        }

        async fn type_text(&mut self, s: &str) {
            for ch in s.chars() {
                self.tap(stroke_for(ch, Layout::En).unwrap().code).await;
            }
        }

        fn ops(&self) -> Vec<Op> {
            self.log
                .borrow()
                .iter()
                .filter_map(|e| match e {
                    Ev::Op(o) => Some(o.clone()),
                    Ev::Call(_) => None,
                })
                .collect()
        }

        fn calls(&self) -> Vec<String> {
            self.log
                .borrow()
                .iter()
                .filter_map(|e| match e {
                    Ev::Call(c) => Some(c.clone()),
                    Ev::Op(_) => None,
                })
                .collect()
        }

        fn has_typing_ops(&self) -> bool {
            self.ops()
                .iter()
                .any(|o| matches!(o, Op::Backspace(_) | Op::Tap(_)))
        }
    }

    fn taps(s: &str) -> Vec<Op> {
        s.chars()
            .map(|c| Op::Tap(stroke_for(c, Layout::En).unwrap()))
            .collect()
    }

    fn correction_ops(word: &str) -> Vec<Op> {
        let mut v = vec![Op::Backspace(word.chars().count() + 1)];
        v.extend(taps(word));
        v.extend(taps(" "));
        v
    }

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // ---- layout_status ----

    #[test]
    fn layout_status_requires_exactly_us_and_ru() {
        assert!(layout_status(&strings(&["us", "ru"])).is_ok());
        assert!(layout_status(&strings(&["ru", "us"])).is_ok());
        assert!(layout_status(&strings(&["us", "de", "ru"])).is_err());
        assert!(layout_status(&strings(&["us", "us", "ru"])).is_err());
        assert!(layout_status(&strings(&["us"])).is_err());
        assert!(layout_status(&[]).is_err());
    }

    // ---- Activity (R1) ----

    #[test]
    fn activity_counts_only_r1_inputs() {
        let a = Activity::new();
        let hk = Config::default().hotkeys().unwrap();
        a.observe(&key_input(30, 1), &hk);
        assert_eq!(a.seq(), 1);
        a.observe(&key_input(30, 2), &hk);
        assert_eq!(a.seq(), 2);
        a.observe(&key_input(30, 0), &hk);
        assert_eq!(a.seq(), 2, "key-up never counts");
        a.observe(&key_input(KEY_SHIFT, 1), &hk);
        assert_eq!(a.seq(), 2, "modifiers never count");
        assert!(a.modifiers_held());
        a.observe(&key_input(KEY_PAUSE, 1), &hk);
        assert_eq!(a.seq(), 2, "Shift+Pause is a hotkey");
        a.observe(&key_input(KEY_SHIFT, 0), &hk);
        assert!(!a.modifiers_held());
        a.observe(&key_input(KEY_PAUSE, 1), &hk);
        assert_eq!(a.seq(), 2, "Pause is a hotkey");
        a.observe(&key_input(29, 1), &hk);
        a.observe(&key_input(KEY_PAUSE, 1), &hk);
        assert_eq!(a.seq(), 3, "Ctrl+Pause is not a hotkey");
        a.observe(&key_input(29, 0), &hk);
        a.observe(&Input::PointerButton, &hk);
        a.observe(&Input::Touch, &hk);
        assert_eq!(a.seq(), 5);
    }

    #[test]
    fn activity_tracks_left_and_right_modifiers_separately() {
        let a = Activity::new();
        let hk = Config::default().hotkeys().unwrap();
        a.observe(&key_input(42, 1), &hk);
        a.observe(&key_input(54, 1), &hk);
        a.observe(&key_input(54, 0), &hk);
        assert!(a.modifiers_held(), "left Shift still down");
        a.observe(&key_input(42, 0), &hk);
        assert!(!a.modifiers_held());
    }

    #[tokio::test(start_paused = true)]
    async fn forward_observes_before_sending() {
        let (in_tx, in_rx) = tokio::sync::mpsc::channel(8);
        let (out_tx, mut out_rx) = tokio::sync::mpsc::channel(8);
        let a = Arc::new(Activity::new());
        tokio::spawn(forward(
            in_rx,
            out_tx,
            a.clone(),
            Config::default().hotkeys().unwrap(),
        ));
        in_tx.send(phys(key_input(30, 1))).await.unwrap();
        let ev = out_rx.recv().await.unwrap();
        assert!(matches!(
            ev.physical.input,
            Input::Key { code: 30, value: 1 }
        ));
        assert_eq!(ev.seq, 1);
        in_tx.send(phys(key_input(30, 0))).await.unwrap();
        assert_eq!(
            out_rx.recv().await.unwrap().seq,
            1,
            "key-up keeps the stamp"
        );
        assert_eq!(a.seq(), 1);
    }

    // ---- automatic correction ----

    #[tokio::test(start_paused = true)]
    async fn corrects_wrong_layout_word() {
        let mut r = rig(Setup::default());
        r.type_text("ghbdtn ").await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"));
        let log = r.log.borrow();
        let set = log
            .iter()
            .position(|e| *e == Ev::Call("set_layout ru".into()))
            .expect("set_layout ru");
        let bs = log
            .iter()
            .position(|e| matches!(e, Ev::Op(Op::Backspace(_))))
            .unwrap();
        assert!(set < bs);
    }

    #[tokio::test(start_paused = true)]
    async fn fresh_corrector_on_used_activity_still_corrects() {
        // E.g. the Corrector was rebuilt after a config reload; Activity lives on.
        let mut r = rig(Setup::default());
        for _ in 0..5 {
            r.activity.observe(&key_input(30, 1), &r.hotkeys);
            r.activity.observe(&key_input(30, 0), &r.hotkeys);
        }
        r.c = rebuilt(&r);
        r.type_text("ghbdtn ").await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"));
    }

    #[tokio::test(start_paused = true)]
    async fn layout_lagging_a_few_polls_still_corrects() {
        let mut r = rig(Setup {
            switch: Switch::Lag(3),
            ..Setup::default()
        });
        r.type_text("ghbdtn ").await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"));
    }

    #[tokio::test(start_paused = true)]
    async fn excluded_app_is_left_alone() {
        let mut r = rig(Setup {
            app: "org.keepassxc.KeePassXC",
            ..Setup::default()
        });
        r.type_text("ghbdtn ").await;
        assert!(r.ops().is_empty());
        assert!(r.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn auto_correct_off_does_nothing() {
        let mut r = rig(Setup {
            config: Config {
                auto_correct: false,
                ..Config::default()
            },
            ..Setup::default()
        });
        r.type_text("ghbdtn ").await;
        assert!(r.ops().is_empty());
        assert!(r.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn unsupported_layout_set_does_nothing() {
        let mut r = rig(Setup {
            layouts: vec!["us", "de", "ru"],
            ..Setup::default()
        });
        r.type_text("ghbdtn ").await;
        assert!(r.ops().is_empty());
        assert!(r.calls().is_empty());
        assert!(layout_status(&strings(&["us", "de", "ru"])).is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn layout_never_switching_injects_nothing() {
        let mut r = rig(Setup {
            switch: Switch::Never,
            ..Setup::default()
        });
        r.type_text("ghbdtn ").await;
        assert!(!r.has_typing_ops());
        assert_eq!(r.calls()[0], "set_layout ru");
    }

    #[tokio::test(start_paused = true)]
    async fn key_during_settle_aborts_and_switches_back() {
        let mut r = rig(Setup::default());
        r.type_text("ghbdtn").await;
        let space = key_input(KEY_SPACE, 1);
        let seq = r.activity.observe(&space, &r.hotkeys);
        let (a, hk) = (r.activity.clone(), r.hotkeys.clone());
        let (res, ()) = tokio::join!(r.c.handle(stamped(space, seq)), async move {
            sleep(Duration::from_millis(20)).await;
            a.observe(&key_input(30, 1), &hk);
        });
        res.unwrap();
        assert!(!r.has_typing_ops());
        assert_eq!(r.calls(), strings(&["set_layout ru", "set_layout us"]));
    }

    #[tokio::test(start_paused = true)]
    async fn already_queued_key_aborts_before_switching() {
        // The user typed past the space before the Corrector saw the space.
        let mut r = rig(Setup::default());
        r.type_text("ghbdtn").await;
        let space = key_input(KEY_SPACE, 1);
        let seq = r.activity.observe(&space, &r.hotkeys);
        r.activity.observe(&key_input(30, 1), &r.hotkeys);
        // The space arrives with its own (now stale) stamp.
        r.c.handle(stamped(space, seq)).await.unwrap();
        assert!(r.ops().is_empty());
        assert!(r.calls().is_empty());
    }

    // ---- Pause: undo and manual convert ----

    #[tokio::test(start_paused = true)]
    async fn pause_right_after_correction_undoes_and_learns() {
        let mut r = rig(Setup::default());
        r.type_text("ghbdtn ").await;
        r.tap(KEY_PAUSE).await;
        let mut want = correction_ops("ghbdtn");
        want.extend(correction_ops("ghbdtn"));
        assert_eq!(r.ops(), want);
        assert_eq!(r.calls(), strings(&["set_layout ru", "set_layout us"]));
        let never = std::fs::read_to_string(r.dir.path().join("never.txt")).unwrap();
        assert_eq!(never.lines().collect::<Vec<_>>(), vec!["ghbdtn"]);
        // Learned: the same word now stays as typed.
        r.type_text("ghbdtn ").await;
        assert_eq!(r.ops(), want);
    }

    #[tokio::test(start_paused = true)]
    async fn pause_after_further_typing_does_not_undo() {
        let mut r = rig(Setup::default());
        r.type_text("ghbdtn ").await;
        let before = r.ops().len();
        r.type_text("f").await;
        r.tap(KEY_PAUSE).await;
        let mut want = vec![Op::Backspace(1)];
        want.extend(taps("f"));
        assert_eq!(r.ops()[before..], want[..]);
        assert!(!r.dir.path().join("never.txt").exists());
    }

    #[tokio::test(start_paused = true)]
    async fn pause_converts_current_word() {
        let mut r = rig(Setup::default());
        r.type_text("ghb").await;
        r.tap(KEY_PAUSE).await;
        let mut want = vec![Op::Backspace(3)];
        want.extend(taps("ghb"));
        assert_eq!(r.ops(), want);
        assert_eq!(r.calls(), strings(&["set_layout ru"]));
    }

    #[tokio::test(start_paused = true)]
    async fn pause_converts_last_word_with_terminator() {
        let mut r = rig(Setup::default());
        // A word the detector keeps, so no automatic correction happens.
        r.type_text("hello ").await;
        assert!(r.ops().is_empty());
        r.tap(KEY_PAUSE).await;
        assert_eq!(r.ops(), correction_ops("hello"));
        assert_eq!(r.calls(), strings(&["set_layout ru"]));
    }

    #[tokio::test(start_paused = true)]
    async fn pause_with_nothing_tracked_does_nothing() {
        let mut r = rig(Setup::default());
        r.tap(KEY_PAUSE).await;
        assert!(r.ops().is_empty());
        assert!(r.calls().is_empty());
    }

    // ---- Shift+Pause: selection convert ----

    async fn shift_pause(r: &mut Rig, release_after: Option<Duration>) {
        r.key(KEY_SHIFT, 1).await;
        let pause = key_input(KEY_PAUSE, 1);
        let seq = r.activity.observe(&pause, &r.hotkeys);
        let (a, hk, log) = (r.activity.clone(), r.hotkeys.clone(), r.log.clone());
        let (res, ()) = tokio::join!(r.c.handle(stamped(pause, seq)), async move {
            if let Some(d) = release_after {
                sleep(d).await;
                assert!(log.borrow().is_empty(), "nothing before Shift is up");
                a.observe(&key_input(KEY_PAUSE, 0), &hk);
                a.observe(&key_input(KEY_SHIFT, 0), &hk);
            }
        });
        res.unwrap();
        r.key(KEY_PAUSE, 0).await;
        r.key(KEY_SHIFT, 0).await;
    }

    #[tokio::test(start_paused = true)]
    async fn selection_waits_for_shift_release() {
        let mut r = rig(Setup::default());
        shift_pause(&mut r, Some(Duration::from_millis(300))).await;
        assert_eq!(
            r.ops(),
            vec![Op::Chord(vec![29], 46), Op::Chord(vec![29], 47)]
        );
        assert_eq!(
            r.calls(),
            strings(&[
                "selection_begin",
                "selection_convert",
                "selection_end",
                "set_layout ru"
            ])
        );
    }

    #[tokio::test(start_paused = true)]
    async fn selection_aborts_if_shift_held_too_long() {
        let mut r = rig(Setup::default());
        shift_pause(&mut r, Some(Duration::from_millis(1100))).await;
        assert!(r.ops().is_empty());
        assert!(r.calls().is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn selection_convert_false_skips_paste() {
        let mut r = rig(Setup {
            convert_target: "",
            ..Setup::default()
        });
        shift_pause(&mut r, Some(Duration::from_millis(50))).await;
        assert_eq!(r.ops(), vec![Op::Chord(vec![29], 46)]);
        assert_eq!(
            r.calls(),
            strings(&["selection_begin", "selection_convert", "selection_end"])
        );
    }

    #[tokio::test(start_paused = true)]
    async fn selection_in_terminal_uses_shifted_chords() {
        let mut r = rig(Setup {
            app: "org.gnome.Ptyxis",
            ..Setup::default()
        });
        shift_pause(&mut r, Some(Duration::from_millis(50))).await;
        assert_eq!(
            r.ops(),
            vec![Op::Chord(vec![29, 42], 46), Op::Chord(vec![29, 42], 47)]
        );
    }

    // ---- final review fixes ----

    fn typing_ops_in(log: &Log) -> bool {
        log.borrow()
            .iter()
            .any(|e| matches!(e, Ev::Op(Op::Backspace(_) | Op::Tap(_))))
    }

    /// Types `word`, then presses Space and keeps it down for `hold`.
    async fn word_with_held_space(r: &mut Rig, word: &str, hold: Duration) {
        r.type_text(word).await;
        let space = key_input(KEY_SPACE, 1);
        let seq = r.activity.observe(&space, &r.hotkeys);
        let (a, hk, log) = (r.activity.clone(), r.hotkeys.clone(), r.log.clone());
        let (res, ()) = tokio::join!(r.c.handle(stamped(space, seq)), async move {
            sleep(hold).await;
            assert!(!typing_ops_in(&log), "nothing injected while Space is down");
            a.observe(&key_input(KEY_SPACE, 0), &hk);
        });
        res.unwrap();
        r.key(KEY_SPACE, 0).await;
    }

    #[test]
    fn activity_tracks_every_held_key() {
        let a = Activity::new();
        let hk = Config::default().hotkeys().unwrap();
        assert!(!a.keys_held());
        a.observe(&key_input(KEY_SPACE, 1), &hk);
        assert!(a.keys_held());
        a.observe(&key_input(KEY_SPACE, 2), &hk);
        a.observe(&key_input(KEY_SHIFT, 1), &hk);
        a.observe(&key_input(KEY_SPACE, 0), &hk);
        assert!(a.keys_held(), "Shift still down");
        a.observe(&key_input(KEY_SHIFT, 0), &hk);
        assert!(!a.keys_held());
        a.observe(&key_input(0x2ff, 1), &hk);
        assert!(a.keys_held(), "KEY_MAX is tracked");
        a.observe(&key_input(0x2ff, 0), &hk);
        assert!(!a.keys_held());
    }

    #[tokio::test(start_paused = true)]
    async fn injection_waits_for_space_release() {
        let mut r = rig(Setup::default());
        word_with_held_space(&mut r, "ghbdtn", Duration::from_millis(150)).await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"));
        assert_eq!(r.calls(), strings(&["set_layout ru"]));
    }

    #[tokio::test(start_paused = true)]
    async fn space_held_too_long_aborts_and_restores() {
        let mut r = rig(Setup::default());
        word_with_held_space(&mut r, "ghbdtn", Duration::from_millis(500)).await;
        assert!(r.ops().is_empty());
        assert_eq!(r.calls(), strings(&["set_layout ru", "set_layout us"]));
    }

    #[tokio::test(start_paused = true)]
    async fn selection_aborts_if_pause_held_too_long() {
        let mut r = rig(Setup::default());
        r.key(KEY_SHIFT, 1).await;
        let pause = key_input(KEY_PAUSE, 1);
        let seq = r.activity.observe(&pause, &r.hotkeys);
        let (a, hk) = (r.activity.clone(), r.hotkeys.clone());
        // Shift comes up soon, but Pause stays down past the key timeout.
        let (res, ()) = tokio::join!(r.c.handle(stamped(pause, seq)), async move {
            sleep(Duration::from_millis(20)).await;
            a.observe(&key_input(KEY_SHIFT, 0), &hk);
        });
        res.unwrap();
        assert!(r.ops().is_empty(), "no chord while Pause is down");
        assert_eq!(r.calls(), strings(&["selection_begin", "selection_end"]));
    }

    #[tokio::test(start_paused = true)]
    async fn typing_during_replay_forgets_the_text() {
        let mut r = rig(Setup::default());
        r.c.injector.disturb = Some((r.activity.clone(), r.hotkeys.clone(), "ghbdtn "));
        r.type_text("ghbdtn ").await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"));
        assert!(r.c.tracker.last_word().is_none());
        assert!(r.c.last_correction.is_none());
        // The keys typed during the replay reach the Corrector afterwards:
        // they must neither be corrected nor become convertible.
        r.deliver_typed().await;
        assert!(r.c.tracker.current_word().is_empty());
        assert!(r.c.tracker.last_word().is_none());
        r.tap(KEY_PAUSE).await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"));
        assert!(!r.dir.path().join("never.txt").exists());
        // Typing after that is tracked normally again (the layout is ru now).
        r.type_text("hello ").await;
        let mut want = correction_ops("ghbdtn");
        want.extend(correction_ops("hello"));
        assert_eq!(r.ops(), want);
    }

    #[tokio::test(start_paused = true)]
    async fn pause_after_idle_reset_does_not_undo() {
        let mut r = rig(Setup::default());
        r.type_text("ghbdtn ").await;
        let later = Instant::now() + Duration::from_secs(6);
        r.tap_at(KEY_PAUSE, later).await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"));
        assert_eq!(r.calls(), strings(&["set_layout ru"]));
        assert!(!r.dir.path().join("never.txt").exists());
    }

    #[tokio::test(start_paused = true)]
    async fn manual_convert_mid_word_is_not_flipped_back() {
        let mut r = rig(Setup::default());
        r.type_text("hello").await;
        r.tap(KEY_PAUSE).await;
        let mut want = vec![Op::Backspace(5)];
        want.extend(taps("hello"));
        assert_eq!(r.ops(), want);
        r.type_text(" ").await;
        assert_eq!(r.ops(), want, "the user's choice stands");
        assert_eq!(r.calls(), strings(&["set_layout ru"]));
        // The next word is auto-corrected as usual (typed in ru now).
        r.type_text("hello ").await;
        assert!(r.ops().len() > want.len());
    }

    #[tokio::test(start_paused = true)]
    async fn no_pointer_device_disables_auto_correct_only() {
        let mut r = rig(Setup {
            pointer: false,
            ..Setup::default()
        });
        r.type_text("ghbdtn ").await;
        assert!(r.ops().is_empty());
        assert!(r.calls().is_empty());
        r.tap(KEY_PAUSE).await;
        assert_eq!(r.ops(), correction_ops("ghbdtn"), "hotkeys still work");
    }

    #[tokio::test(start_paused = true)]
    async fn selection_begin_failure_still_ends() {
        let mut r = rig(Setup {
            begin_fails: true,
            ..Setup::default()
        });
        shift_pause(&mut r, Some(Duration::from_millis(50))).await;
        assert!(r.ops().is_empty());
        assert_eq!(r.calls(), strings(&["selection_begin", "selection_end"]));
    }

    #[tokio::test(start_paused = true)]
    async fn selection_switches_to_the_converted_texts_layout() {
        // Layout is us, but the selection was Cyrillic: it is now Latin.
        let mut r = rig(Setup {
            convert_target: "us",
            ..Setup::default()
        });
        shift_pause(&mut r, Some(Duration::from_millis(50))).await;
        assert_eq!(
            r.calls(),
            strings(&[
                "selection_begin",
                "selection_convert",
                "selection_end",
                "set_layout us"
            ])
        );
    }

    #[tokio::test(start_paused = true)]
    async fn injector_error_forgets_text_but_keeps_caps() {
        let mut r = rig(Setup::default());
        r.type_text("ghbdtn").await;
        r.c.tracker.set_caps(true);
        r.c.injector.fail = true;
        let (down, up) = (key_input(KEY_SPACE, 1), key_input(KEY_SPACE, 0));
        let seq = r.activity.observe(&down, &r.hotkeys);
        r.activity.observe(&up, &r.hotkeys);
        assert!(r.c.handle(stamped(down, seq)).await.is_err());
        r.c.injector.fail = false;
        assert!(r.c.tracker.last_word().is_none());
        assert!(r.c.tracker.current_word().is_empty());
        r.key(KEY_SPACE, 0).await;
        r.type_text("a").await;
        assert!(r.c.tracker.current_word()[0].caps, "Caps Lock state kept");
    }
}
