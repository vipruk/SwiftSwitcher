//! Correction orchestration: decides when to erase and retype the user's text.
//!
//! The overriding rule is to never corrupt text. Whenever anything is uncertain
//! (the layout did not switch, the user kept typing, modifiers are held, a D-Bus
//! call failed) the action is abandoned before anything is injected: the layout
//! is switched and confirmed first, and only then are keys sent.
//! Log lines carry only lengths, verdict reasons, layout ids and app ids.

#![allow(dead_code)] // wired up by the daemon entry point

use crate::config::Config;
use crate::desktop::Desktop;
use crate::inject::Injector;
use crate::input::Physical;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
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
/// for the Corrector (see [`forward`]). The Corrector compares `seq` with the
/// number of activity events it has handled itself: a difference means the
/// user did something it has not processed yet, and it must not inject.
///
/// Single writer: only one task may call `observe`.
#[derive(Default)]
pub struct Activity {
    seq: AtomicU64,
    mods: AtomicU16,
}

impl Activity {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn observe(&self, input: &Input, hotkeys: &[(Hotkey, HotkeyAction)]) {
        let (state, counted) = step(self.mods.load(Ordering::SeqCst), input, hotkeys);
        self.mods.store(state, Ordering::SeqCst);
        if counted {
            self.seq.fetch_add(1, Ordering::SeqCst);
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
}

/// Moves events from the reader to the Corrector, observing each one first.
/// Every event must pass through here, unfiltered, or the Corrector will see
/// unprocessed activity forever and stop acting.
pub async fn forward(
    mut rx: Receiver<Physical>,
    tx: Sender<Physical>,
    activity: Arc<Activity>,
    hotkeys: Vec<(Hotkey, HotkeyAction)>,
) {
    while let Some(p) = rx.recv().await {
        activity.observe(&p.input, &hotkeys);
        if tx.send(p).await.is_err() {
            break;
        }
    }
}

// ---- corrector ----

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
    hotkeys: Vec<(Hotkey, HotkeyAction)>,
    activity: Arc<Activity>,
    /// Modifier state as of the last handled event (same rules as `Activity`).
    mods: u16,
    /// Activity events handled so far; equals `activity.seq()` when idle.
    handled: u64,
    last_correction: Option<Correction>,
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
        let mut tracker =
            Tracker::new(hotkeys.clone(), Duration::from_secs(config.idle_reset_secs));
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
            hotkeys,
            activity,
            mods: 0,
            handled: 0,
            last_correction: None,
        })
    }

    /// Feeds one event (already passed to `Activity::observe`) and acts on it.
    pub async fn handle(&mut self, p: Physical) -> Result<()> {
        let (mods, counted) = step(self.mods, &p.input, &self.hotkeys);
        self.mods = mods;
        if counted {
            self.handled += 1;
            self.last_correction = None;
        }
        match self.tracker.feed(p.input, p.at) {
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
        self.activity.seq() == self.handled
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
                if self
                    .correct(&word, Some(terminator), erase, from, from.other())
                    .await?
                {
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

    /// Switches to `to`, confirms it, waits `settle_ms`, then erases `erase`
    /// characters and retypes `strokes` (and `terminator`). Returns `Ok(false)`
    /// if abandoned, in which case nothing was injected and the layout was put
    /// back to `from`.
    async fn correct(
        &mut self,
        strokes: &[Stroke],
        terminator: Option<Stroke>,
        erase: usize,
        from: Layout,
        to: Layout,
    ) -> Result<bool> {
        if !self.quiet() || !self.wait_modifiers_released().await {
            return Ok(false);
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
            return Ok(false);
        }
        sleep(Duration::from_millis(self.config.settle_ms)).await;
        if !self.quiet() || self.activity.modifiers_held() {
            log::debug!("input arrived during settle; correction abandoned");
            self.restore(from).await;
            return Ok(false);
        }
        if let Err(e) = self.inject(strokes, terminator, erase) {
            // The text near the caret is now unknown; forget everything.
            self.tracker = Tracker::new(
                self.hotkeys.clone(),
                Duration::from_secs(self.config.idle_reset_secs),
            );
            self.last_correction = None;
            return Err(e);
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
        Ok(true)
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
            return self.undo(c).await;
        }
        let Some(from) = self.supported_layout().await else {
            return Ok(());
        };
        let to = from.other();
        if !self.tracker.current_word().is_empty() {
            let word = self.tracker.current_word().to_vec();
            self.correct(&word, None, word.len(), from, to).await?;
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
        if !undone {
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
        match self.desktop.selection_begin().await {
            Ok(true) => {}
            Ok(false) => {
                log::debug!("selection convert refused");
                return Ok(());
            }
            Err(e) => {
                log::warn!("cannot begin selection convert: {e:#}");
                return Ok(());
            }
        }
        let pasted = self.copy_convert_paste(mods).await;
        if let Err(e) = self.desktop.selection_end().await {
            log::warn!("cannot end selection convert: {e:#}");
        }
        if pasted? {
            let to = source_id(from.other());
            match self.desktop.set_layout(to).await {
                Ok(true) => log::info!("converted selection; input source {to}"),
                Ok(false) => log::warn!("input source {to} was refused"),
                Err(e) => log::warn!("cannot switch input source to {to}: {e:#}"),
            }
        }
        Ok(())
    }

    /// Copy, convert the clipboard, paste. `Ok(true)` iff it pasted.
    async fn copy_convert_paste(&mut self, mods: &[KeyCode]) -> Result<bool> {
        self.injector.chord(mods, KEY_C)?;
        sleep(COPY_WAIT).await;
        match self.desktop.selection_convert().await {
            Ok(true) => {}
            Ok(false) => {
                log::debug!("nothing to convert in the selection");
                return Ok(false);
            }
            Err(e) => {
                log::warn!("cannot convert selection: {e:#}");
                return Ok(false);
            }
        }
        if !self.quiet() || self.activity.modifiers_held() {
            log::debug!("input arrived during selection convert; not pasting");
            return Ok(false);
        }
        self.injector.chord(mods, KEY_V)?;
        sleep(PASTE_WAIT).await;
        Ok(true)
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
        convert_ok: bool,
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
            Ok(true)
        }
        async fn selection_convert(&self) -> Result<bool> {
            self.call("selection_convert");
            Ok(self.convert_ok)
        }
        async fn selection_end(&self) -> Result<bool> {
            self.call("selection_end");
            Ok(true)
        }
    }

    struct FakeInjector {
        log: Log,
    }

    impl FakeInjector {
        fn op(&mut self, op: Op) -> anyhow::Result<()> {
            self.log.borrow_mut().push(Ev::Op(op));
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
        convert_ok: bool,
        config: Config,
    }

    impl Default for Setup {
        fn default() -> Self {
            Setup {
                layouts: vec!["us", "ru"],
                app: "org.gnome.TextEditor",
                switch: Switch::Now,
                convert_ok: true,
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
            convert_ok: s.convert_ok,
        };
        let injector = FakeInjector { log: log.clone() };
        let activity = Arc::new(Activity::new());
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

    fn key_input(code: KeyCode, value: i32) -> Input {
        Input::Key { code, value }
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
            self.activity.observe(&input, &self.hotkeys);
            self.c.handle(phys(input)).await.unwrap();
        }

        async fn key(&mut self, code: KeyCode, value: i32) {
            self.feed(key_input(code, value)).await;
        }

        async fn tap(&mut self, code: KeyCode) {
            self.key(code, 1).await;
            self.key(code, 0).await;
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
        let p = out_rx.recv().await.unwrap();
        assert!(matches!(p.input, Input::Key { code: 30, value: 1 }));
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
        r.activity.observe(&space, &r.hotkeys);
        let (a, hk) = (r.activity.clone(), r.hotkeys.clone());
        let (res, ()) = tokio::join!(r.c.handle(phys(space)), async move {
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
        r.activity.observe(&space, &r.hotkeys);
        r.activity.observe(&key_input(30, 1), &r.hotkeys);
        r.c.handle(phys(space)).await.unwrap();
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
        r.activity.observe(&pause, &r.hotkeys);
        let (a, hk, log) = (r.activity.clone(), r.hotkeys.clone(), r.log.clone());
        let (res, ()) = tokio::join!(r.c.handle(phys(pause)), async move {
            if let Some(d) = release_after {
                sleep(d).await;
                assert!(log.borrow().is_empty(), "nothing before Shift is up");
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
            convert_ok: false,
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
}
