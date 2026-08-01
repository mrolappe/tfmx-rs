//! The macro interpreter: one voice's macro program (opcodes `$00`-`$29`).
//! `docs/opcodes.md` §3, `docs/playback-model.md` §4-§6.

use crate::module::{AccessError, Module};
use crate::paula::Paula;

/// Paula's PAL reference clock, matching `paula.rs`'s private constant.
/// `docs/playback-model.md` §2.1.
const PAULA_CLOCK_HZ: f64 = 3_546_895.0;

/// [S1] states this anchor at note `$1E`, but a real-editor A/B (a from-
/// scratch minimal scale, `testdata/synth/`) measured the editor a tritone
/// (6 semitones) higher than that for identical note bytes -- `$18` is the
/// anchor that matches the editor. `docs/macro-playback-fidelity.md` §14,
/// `docs/playback-model.md` §4.
const MIDDLE_C_NOTE: i32 = 0x18;
const MIDDLE_C_HZ: f64 = 8363.0;

/// Paula period for `note` (already transposed -- may fall outside `$00`-
/// `$3F`, the formula extrapolates) with a Q8.8-style finetune applied to
/// frequency: `multiplier = 1 + finetune/256`. Shared by the pattern
/// record's 8-bit detune and the macro opcodes' 16-bit finetune -- both are
/// the same convention at two widths, so the two are summed (saturating: the
/// 16-bit operand is raw module data) before reaching here.
/// `docs/playback-model.md` §4, §4.2.
pub(crate) fn note_period(note: i32, finetune: i16) -> u16 {
    let freq = MIDDLE_C_HZ * 2f64.powf((note - MIDDLE_C_NOTE) as f64 / 12.0);
    let multiplier = 1.0 + (finetune as f64 / 256.0);
    let period = PAULA_CLOCK_HZ / (freq * multiplier);
    period.round().clamp(0.0, u16::MAX as f64) as u16
}

/// Sign-extends a 24-bit big-endian value (as used by `$02`/`$18`) to `i32`.
fn sext24(hi: u8, mid: u8, lo: u8) -> i32 {
    let raw = ((hi as u32) << 16) | ((mid as u32) << 8) | (lo as u32);
    if raw & 0x0080_0000 != 0 {
        (raw | 0xFF00_0000) as i32
    } else {
        raw as i32
    }
}

/// A `[u32; 256]` counter table indexed by raw macro opcode byte, for
/// opcodes this crate recognizes but does not implement (`$1B`, `$22`-`$29`)
/// -- "record, never guess", `docs/opcodes.md` Unresolved section.
#[derive(Debug, Clone)]
pub struct UnsupportedOps(Box<[u32; 256]>);

impl Default for UnsupportedOps {
    fn default() -> Self {
        Self(Box::new([0; 256]))
    }
}

impl UnsupportedOps {
    fn count(&mut self, opcode: u8) {
        self.0[opcode as usize] += 1;
    }

    pub fn get(&self, opcode: u8) -> u32 {
        self.0[opcode as usize]
    }
}

/// `$0F <Envelope>` / `$F7 <Enve>` / `$FA <Fade>` shape: every `every`
/// jiffies, `value` moves by `amount` towards `target`, clamped on arrival.
/// `docs/playback-model.md` §5.1.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Envelope {
    amount: u8,
    every: u8,
    target: u8,
    counter: u8,
}

impl Envelope {
    pub(crate) fn new(amount: u8, every: u8, target: u8) -> Self {
        Self {
            amount,
            every,
            target,
            counter: every,
        }
    }

    /// Advances one jiffy. Returns `false` once `volume` has reached
    /// `target` -- the caller drops the envelope on `false`.
    pub(crate) fn tick(&mut self, volume: &mut u8) -> bool {
        if self.every != 0 {
            self.counter = self.counter.wrapping_sub(1);
            if self.counter != 0 {
                return true;
            }
            self.counter = self.every;
        }
        let diff = self.target as i16 - *volume as i16;
        if diff == 0 {
            return false;
        }
        let step = self.amount as i16 * diff.signum();
        let next = *volume as i16 + step;
        *volume = if diff > 0 {
            next.min(self.target as i16)
        } else {
            next.max(self.target as i16)
        } as u8;
        *volume != self.target
    }
}

/// `$0B <Portamento>` / `$FC <Port>`: every `every` jiffies, multiply the
/// period by `(256+rate)/256`. `docs/playback-model.md` §5.3.
#[derive(Debug, Clone, Copy)]
struct Portamento {
    every: u8,
    rate: i16,
    counter: u8,
}

impl Portamento {
    fn new(every: u8, rate: i16) -> Self {
        Self {
            every,
            rate,
            counter: every,
        }
    }

    fn tick(&mut self, period: &mut u16) {
        if self.every != 0 {
            self.counter = self.counter.wrapping_sub(1);
            if self.counter != 0 {
                return;
            }
            self.counter = self.every;
        }
        // Truncate, not round: docs/playback-model.md §5.3's stated
        // preference, kept consistent every step since rounding differences
        // compound over many jiffies.
        let scaled = *period as f64 * (256.0 + self.rate as f64) / 256.0;
        *period = scaled.trunc().clamp(0.0, u16::MAX as f64) as u16;
    }
}

/// `$0C <Vibrato>` / `$F6 <Vibr>`: a period-domain triangle LFO, quarter-
/// phase reading. `docs/playback-model.md` §5.2.
#[derive(Debug, Clone, Copy)]
struct Vibrato {
    half_period: u8,
    slide: i8,
    t: u16,
}

impl Vibrato {
    fn new(half_period: u8, slide: i8) -> Self {
        Self {
            half_period,
            slide,
            t: 0,
        }
    }

    /// This jiffy's signed period delta; advances the phase by one jiffy.
    fn delta(&mut self) -> i32 {
        let period = 2 * self.half_period as u32;
        if period == 0 {
            return 0;
        }
        let t = self.t as u32 % period;
        let half = self.half_period as u32 / 2;
        let slide = self.slide as i32;
        let d = if t < half {
            slide * t as i32
        } else if t < half + self.half_period as u32 {
            slide * (half as i32 - (t as i32 - half as i32))
        } else {
            -slide * (period as i32 - t as i32)
        };
        self.t = self.t.wrapping_add(1);
        d
    }
}

/// `$11 <AddBegin>` in its periodic form (`aa != 0`): a triangle ramp on the
/// sample pointer, `0 -> aa*step -> 0` over `2*aa` jiffies.
/// `docs/opcodes.md` §3.
#[derive(Debug, Clone, Copy)]
struct PointerVibrato {
    half_period: u8,
    step: i32,
    t: u16,
}

impl PointerVibrato {
    fn delta(&mut self) -> i32 {
        let cycle = 2 * self.half_period as u32;
        if cycle == 0 {
            return 0;
        }
        let t = self.t as u32 % cycle;
        let d = if t <= self.half_period as u32 {
            self.step * t as i32
        } else {
            self.step * (cycle as i32 - t as i32)
        };
        self.t = self.t.wrapping_add(1);
        d
    }
}

/// Suspend reason for the macro program counter. Effects (vibrato,
/// portamento, envelope, pointer vibrato) tick regardless of this state --
/// they run "independently of the macro program counter",
/// `docs/playback-model.md` §5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wait {
    Ready,
    /// `n` more jiffies to skip after this one.
    Jiffies(u16),
    /// `$14 <Wait key up>`: `None` = indefinite (`aa` = 0).
    KeyUp(Option<u16>),
    /// `$1A <Wait on DMA>`: resumes once `Paula::loop_completions` reaches
    /// this target.
    DmaCompletions(u32),
    /// `$07 <STOP>`: cleared only by `MacroInterpreter::trigger`.
    Stopped,
}

/// An effect a macro opcode causes outside its own voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroEvent {
    /// `$21 <Play macro>`: start macro `macro_number` on `channel` with
    /// `detune`.
    PlayMacro {
        channel: u8,
        macro_number: u8,
        detune: i8,
    },
}

/// Entries executed in a single jiffy before the interpreter gives up --
/// mirrors `PatternRunner`'s `MAX_PATTERN_ENTRIES_PER_JIFFY` bound for the
/// same reason: waitless loop data must not hang the player.
const MAX_MACRO_OPS_PER_JIFFY: usize = 1024;

/// One voice's macro program: PC, wait state, note/transpose/volume
/// context, the running effects, and the Paula register shadow a fresh
/// program builds up before it is ever written to `Paula`.
/// `docs/opcodes.md` §3, `docs/playback-model.md` §4-§6.
#[derive(Debug)]
pub struct MacroInterpreter {
    macro_number: u8,
    /// The macro number a pattern's Note event last triggered this voice
    /// with -- unlike `macro_number`, `$06 <Cont>` never changes this. A
    /// keysplit instrument's public macro dispatches via `$1C`/`$06` into a
    /// different macro number within the *same* jiffy as the trigger, so
    /// by the next Note event `macro_number` has already moved on;
    /// `note_on` needs the pattern's own number to recognize "the same
    /// instrument is still running".
    instrument: u8,
    step: u16,
    wait: Wait,
    saved: Option<(u8, u16)>,
    repeat: Option<u8>,
    key_up: bool,

    note: i32,
    last_note: i32,
    transpose: i8,
    /// Extra finetune from `$21 <Play macro>`'s `detune` operand, added into
    /// the next `$08`/`$09`/`$1F`'s finetune. **Uncertain**: [S1] gives $21
    /// one line ("starts macro aa on channel b with detune cc") with no
    /// worked example; this crate's reading, not a cited fact.
    detune: i16,

    volume: u8,
    period: u16,
    dma_on: bool,
    sample_start: u32,
    sample_len: u32,
    loop_start: u32,
    loop_len: u32,
    /// Set once `$18 <Sampleloop>` has run: from then on the voice is
    /// reading `loop_start`/`loop_len`, not `sample_start`/`sample_len`
    /// (a stale pre-loop snapshot), so any further pointer nudge (`$11
    /// <AddBegin>`) must target the loop pointer instead.
    loop_active: bool,

    vibrato: Option<Vibrato>,
    portamento: Option<Portamento>,
    envelope: Option<Envelope>,
    pointer_vibrato: Option<PointerVibrato>,

    /// `$20 <Signal>`: recognized and stored, like pattern `MasterVolSlide`
    /// -- nothing in this crate consumes signals yet.
    signals: [u16; 4],
}

impl Default for MacroInterpreter {
    fn default() -> Self {
        Self {
            macro_number: 0,
            instrument: 0,
            step: 0,
            wait: Wait::Stopped,
            saved: None,
            repeat: None,
            key_up: false,
            note: 0,
            last_note: 0,
            transpose: 0,
            detune: 0,
            volume: 0,
            period: 0,
            dma_on: false,
            sample_start: 0,
            sample_len: 0,
            loop_start: 0,
            loop_len: 0,
            loop_active: false,
            vibrato: None,
            portamento: None,
            envelope: None,
            pointer_vibrato: None,
            signals: [0; 4],
        }
    }
}

impl MacroInterpreter {
    pub fn new() -> Self {
        Self::default()
    }

    /// The macro program currently running.
    pub fn macro_number(&self) -> u8 {
        self.macro_number
    }

    /// Whether `$07 <STOP>` has parked this voice's macro program.
    pub fn is_stopped(&self) -> bool {
        self.wait == Wait::Stopped
    }

    /// (Re)starts macro `macro_number` at step 0 for `note` at relative
    /// volume `volume` (`$0`-`$F`, per the pattern note record), transposed
    /// by the track's `transpose`. Clears every running effect and the
    /// sample-pointer shadow -- a fresh macro program builds its own state
    /// up from `$00`/`$02`/`$03` as real macro data does.
    pub fn trigger(&mut self, macro_number: u8, note: u8, volume: u8, transpose: i8) {
        self.last_note = self.note;
        self.note = note as i32;
        self.transpose = transpose;
        self.detune = 0;
        self.volume = volume.min(15) * 3;
        self.macro_number = macro_number;
        self.instrument = macro_number;
        self.step = 0;
        self.wait = Wait::Ready;
        self.saved = None;
        self.repeat = None;
        self.key_up = false;
        self.period = 0;
        self.dma_on = false;
        self.sample_start = 0;
        self.sample_len = 0;
        self.loop_start = 0;
        self.loop_len = 0;
        self.loop_active = false;
        self.vibrato = None;
        self.portamento = None;
        self.envelope = None;
        self.pointer_vibrato = None;
    }

    /// A pattern's `Note` command. If `macro_number` is the instrument
    /// already running on this voice (per the last `trigger()`, not
    /// necessarily `self.macro_number` -- a keysplit instrument's `$06
    /// <Cont>` moves the program counter to a different macro number
    /// within the same jiffy as the trigger) and hasn't reached `$07
    /// <STOP>` (or been silenced by `$FE`), this updates the note/volume in
    /// place instead of restarting the program at step 0 -- a fast note run
    /// that keeps retriggering the same instrument would otherwise never
    /// survive past `$00 aa=0`'s mandatory 1-jiffy pause to reach its own
    /// `$01 DMAon`. **Uncertain**: no [S1] citation states this; grounded
    /// empirically by an A/B against `uade123` on `turrican intro`'s voice 1
    /// (`docs/status.md`), not by the published spec.
    ///
    /// `detune` is the pattern note record's `dd` byte (`NoteTiming::Detune`,
    /// only present when the note byte is below `$80`); like `$21`'s detune it
    /// is folded into the next `$08`/`$09`/`$1E`/`$1F`'s finetune.
    pub fn note_on(&mut self, macro_number: u8, note: u8, volume: u8, transpose: i8, detune: i8) {
        if macro_number == self.instrument && !self.is_stopped() {
            self.last_note = self.note;
            self.note = note as i32;
            self.transpose = transpose;
            self.volume = volume.min(15) * 3;
        } else {
            self.trigger(macro_number, note, volume, transpose);
        }
        // After `trigger`, which clears it: this note's own finetune replaces
        // whatever a previous `$21` left behind. `docs/playback-model.md` §4.
        self.detune = detune as i16;
    }

    /// `$21 <Play macro>`: starts `macro_number` at step 0 with `detune`
    /// folded into the next `$08`/`$09`/`$1F`'s finetune. Unlike
    /// `MacroInterpreter::trigger`, this keeps the voice's current note,
    /// transpose and volume -- [S1] gives $21 one line ("starts macro aa on
    /// channel b with detune cc") with no note operand of its own, so a full
    /// reset to note/volume 0 would silently invent behavior $21 never
    /// asked for. **Uncertain**: this crate's reading, not a cited fact.
    pub fn play_macro(&mut self, macro_number: u8, detune: i8) {
        self.macro_number = macro_number;
        self.step = 0;
        self.wait = Wait::Ready;
        self.saved = None;
        self.repeat = None;
        self.detune = detune as i16;
    }

    /// `$F5 <Kup^>`: sets the release flag. Wakes the program if it is
    /// suspended in `$14 <Wait key up>`; otherwise recorded for `$10 <Loop
    /// key up>` to observe, with no other effect -- `docs/opcodes.md` §2.
    pub fn signal_key_up(&mut self) {
        self.key_up = true;
        if matches!(self.wait, Wait::KeyUp(_)) {
            self.wait = Wait::Ready;
        }
    }

    /// The trackstep per-track word's `$FE` value ("stop the voice in the
    /// low byte", `docs/opcodes.md` §1) -- silences the voice the same way
    /// `$00 <DMAoff+Reset>` with `aa = 0` does, then parks the program as
    /// `$07 <STOP>` would, until a new note retriggers it.
    pub fn stop_voice(&mut self) {
        self.dma_on = false;
        self.reset_effects();
        self.wait = Wait::Stopped;
    }

    /// `$0B <Portamento>` / `$FC <Port>`: shared by the macro opcode and the
    /// pattern command that targets an arbitrary voice. `Portamento::tick`
    /// always multiplies `self.period` in place, so "if not already
    /// running, the current period is loaded in as the starting point"
    /// (`docs/opcodes.md` §3) holds automatically -- there is no separate
    /// starting-point field to seed.
    pub fn start_portamento(&mut self, every: u8, rate: i16) {
        self.portamento = Some(Portamento::new(every, rate));
    }

    /// `$0C <Vibrato>` / `$F6 <Vibr>`.
    pub fn start_vibrato(&mut self, half_period: u8, slide: i8) {
        self.vibrato = Some(Vibrato::new(half_period, slide));
    }

    /// `$0F <Envelope>` / `$F7 <Enve>`.
    pub fn start_envelope(&mut self, amount: u8, every: u8, target: u8) {
        self.envelope = Some(Envelope::new(amount, every, target));
    }

    /// `$0A <Reset>`: stops frequency/pointer vibrato, portamento and the
    /// volume envelope. `docs/opcodes.md` §3.
    fn reset_effects(&mut self) {
        self.vibrato = None;
        self.portamento = None;
        self.envelope = None;
        self.pointer_vibrato = None;
    }

    fn fetch(&self, module: &Module) -> Result<[u8; 4], AccessError> {
        let data = module.macro_(self.macro_number)?;
        let start = self.step as usize * 4;
        let bytes = data.get(start..start + 4).ok_or(AccessError::OutOfRange)?;
        Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
    }

    /// Advances suspend state by one jiffy. Returns whether opcodes should
    /// be fetched this jiffy.
    fn take_turn(&mut self, paula: &mut Paula, voice: u8) -> bool {
        match self.wait {
            Wait::Ready => true,
            Wait::Jiffies(0) => {
                self.wait = Wait::Ready;
                true
            }
            Wait::Jiffies(n) => {
                self.wait = Wait::Jiffies(n - 1);
                false
            }
            Wait::KeyUp(deadline) => {
                if self.key_up {
                    self.key_up = false;
                    self.wait = Wait::Ready;
                    true
                } else {
                    match deadline {
                        Some(0) => {
                            self.wait = Wait::Ready;
                            true
                        }
                        Some(n) => {
                            self.wait = Wait::KeyUp(Some(n - 1));
                            false
                        }
                        None => false,
                    }
                }
            }
            Wait::DmaCompletions(target) => {
                if paula.loop_completions(voice) >= target {
                    self.wait = Wait::Ready;
                    true
                } else {
                    false
                }
            }
            Wait::Stopped => false,
        }
    }

    /// Advances one jiffy: runs the free-running effects, then the macro
    /// program if it is not suspended, then commits the resulting register
    /// state to `paula`. `emit` receives cross-voice events (`$21`).
    pub fn tick(
        &mut self,
        module: &Module,
        paula: &mut Paula,
        voice: u8,
        unsupported: &mut UnsupportedOps,
        mut emit: impl FnMut(MacroEvent),
    ) -> Result<(), AccessError> {
        if let Some(p) = &mut self.portamento {
            p.tick(&mut self.period);
        }
        if let Some(e) = &mut self.envelope
            && !e.tick(&mut self.volume)
        {
            self.envelope = None;
        }

        if self.take_turn(paula, voice) {
            for _ in 0..MAX_MACRO_OPS_PER_JIFFY {
                let bytes = self.fetch(module)?;
                if !self.execute(bytes, paula, voice, unsupported, &mut emit) {
                    break;
                }
            }
        }

        let period = match &mut self.vibrato {
            Some(v) => self.period.saturating_add_signed(
                v.delta().clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16,
            ),
            None => self.period,
        };
        let (sample_start, sample_len) = match &mut self.pointer_vibrato {
            Some(pv) => {
                let delta = pv.delta();
                if self.loop_active {
                    self.loop_start = self.loop_start.wrapping_add_signed(delta);
                    (self.sample_start, self.sample_len)
                } else {
                    (self.sample_start.wrapping_add_signed(delta), self.sample_len)
                }
            }
            None => (self.sample_start, self.sample_len),
        };

        paula.set_period(voice, period);
        paula.set_volume(voice, self.volume);
        paula.set_sample_region(voice, sample_start, sample_len);
        paula.set_loop_region(voice, self.loop_start, self.loop_len);
        paula.set_dma(voice, self.dma_on);
        Ok(())
    }

    /// Executes one opcode. Returns whether the interpreter should keep
    /// fetching opcodes within this same jiffy.
    fn execute(
        &mut self,
        bytes: [u8; 4],
        paula: &mut Paula,
        voice: u8,
        unsupported: &mut UnsupportedOps,
        emit: &mut impl FnMut(MacroEvent),
    ) -> bool {
        let [op, b1, b2, b3] = bytes;
        let word23 = u16::from_be_bytes([b2, b3]);
        self.step += 1;

        match op {
            0x00 => {
                // <DMAoff+Reset>*
                self.dma_on = false;
                self.reset_effects();
                if b1 != 0 {
                    true
                } else {
                    self.wait = Wait::Jiffies(0);
                    false
                }
            }
            0x01 => {
                // <DMAon>
                self.dma_on = true;
                true
            }
            0x02 => {
                // <SetBegin>. `docs/playback-model.md` §2's own table
                // states this targets an "absolute smpl offset" -- unlike
                // `$18 Sampleloop`'s explicitly-documented additive,
                // non-idempotent delta (§7 gotchas). A second `$02` within
                // one trigger (a common corpus idiom -- see
                // `set_begin_is_absolute_not_cumulative`) replaces the
                // pointer rather than adding onto it.
                self.sample_start = (sext24(b1, b2, b3) as u32) & 0x00FF_FFFF;
                self.loop_start = self.sample_start;
                self.loop_len = self.sample_len;
                true
            }
            0x03 => {
                // <SetLen>
                self.sample_len = word23 as u32;
                self.loop_start = self.sample_start;
                self.loop_len = self.sample_len;
                true
            }
            0x04 => {
                // <Wait>*: waits `word23` jiffies (no "+1" -- docs/opcodes.md §3).
                // Asterisked, so it always suspends for at least one jiffy
                // (docs/opcodes.md §3 intro) -- same convention as `$00`'s own
                // `aa == 0` case -- even when `word23 == 0`.
                self.wait = Wait::Jiffies(word23.saturating_sub(1));
                false
            }
            0x05 | 0x10 => {
                // <Loop> / <Loop key up>
                if op == 0x10 && self.key_up {
                    return true;
                }
                let times = b1;
                let target = word23;
                if times == 0 {
                    // Unconditional jump, no counting -- must not touch
                    // `self.repeat` via `get_or_insert`, which would plant a
                    // stale `Some(0)` for the next *counted* loop instruction
                    // (a different `$05`, sharing this same field) to
                    // misread as "already exhausted".
                    self.repeat = None;
                    self.step = target;
                } else {
                    let left = *self.repeat.get_or_insert(times);
                    if left == 0 {
                        self.repeat = None;
                    } else {
                        self.repeat = Some(left - 1);
                        self.step = target;
                    }
                }
                true
            }
            0x06 => {
                // <Cont>
                self.macro_number = b1;
                self.step = word23;
                true
            }
            0x07 => {
                // <STOP>*
                self.wait = Wait::Stopped;
                false
            }
            0x08 => {
                // <AddNote>*
                let note = self.note + b1 as i8 as i32 + self.transpose as i32;
                self.period = note_period(note, (word23 as i16).saturating_add(self.detune));
                self.wait = Wait::Jiffies(0);
                false
            }
            0x09 => {
                // <SetNote>*
                let note = b1 as i32 + self.transpose as i32;
                self.period = note_period(note, (word23 as i16).saturating_add(self.detune));
                self.wait = Wait::Jiffies(0);
                false
            }
            0x0A => {
                // <Reset>
                self.reset_effects();
                true
            }
            0x0B => {
                // <Portamento>
                self.start_portamento(b1, word23 as i16);
                true
            }
            0x0C => {
                // <Vibrato>
                self.start_vibrato(b1, b3 as i8);
                true
            }
            0x0D => {
                // <AddVolume>
                self.volume = (self.volume as i16 + b3 as i8 as i16).clamp(0, 64) as u8;
                true
            }
            0x0E => {
                // <SetVolume>
                self.volume = b1.min(64);
                true
            }
            0x0F => {
                // <Envelope>
                self.start_envelope(b1, b2, b3);
                true
            }
            0x11 => {
                // <AddBegin> -- pointer vibrato. Targets whichever pointer
                // is actually live: the loop pointer once `$18` has handed
                // playback off to it, the pre-loop attack pointer before
                // that (see `loop_active`'s doc comment).
                let step = i16::from_be_bytes([b2, b3]) as i32;
                if b1 == 0 {
                    if self.loop_active {
                        self.loop_start = self.loop_start.wrapping_add_signed(step);
                    } else {
                        self.sample_start = self.sample_start.wrapping_add_signed(step);
                    }
                    self.pointer_vibrato = None;
                } else {
                    self.pointer_vibrato = Some(PointerVibrato {
                        half_period: b1,
                        step,
                        t: 0,
                    });
                }
                true
            }
            0x12 => {
                // <AddLen>. Paula's length register is 16-bit hardware
                // (`docs/format.md` §8) -- mask to that width so an overflow
                // wraps mod 65536 like the real chip, not mod 2^32.
                self.sample_len = self.sample_len.wrapping_add(word23 as u32) & 0xFFFF;
                true
            }
            0x13 => {
                // <DMAoff>*
                self.dma_on = false;
                true
            }
            0x14 => {
                // <Wait key up>*
                let deadline = if b3 == 0 { None } else { Some(b3 as u16 - 1) };
                self.wait = Wait::KeyUp(deadline);
                false
            }
            0x15 => {
                // <Go submacro>
                self.saved = Some((self.macro_number, self.step));
                self.macro_number = b1;
                self.step = word23;
                true
            }
            0x16 => {
                // <Return to old macro>
                if let Some((m, s)) = self.saved.take() {
                    self.macro_number = m;
                    self.step = s;
                }
                true
            }
            0x17 => {
                // <Set period>*
                self.period = word23;
                self.wait = Wait::Jiffies(0);
                false
            }
            0x18 => {
                // <Sampleloop>. `docs/opcodes.md` §4: the 24-bit delta is
                // added to `loop_start` (a *byte* address, same units as
                // `$02 SetBegin`) and subtracted from `loop_len` -- but
                // `loop_len` mirrors Paula's length register, which counts
                // *words* (`docs/format.md` §8, one count = 2 bytes, same as
                // `$03 SetLen`). Applying the byte-valued delta directly to
                // the word-valued length breaks the doc's own stated
                // invariant ("the sample's end point is unchanged"): halving
                // it first is what keeps `loop_start + loop_len*2` constant.
                // An arithmetic shift (not `/2`) matches a real 68000's ASR
                // idiom for negative deltas, consistent with the rounding
                // convention already adopted elsewhere (`docs/playback-
                // model.md` §5.3). When the halved delta exceeds the current
                // loop_len, mask to 16 bits so the subtraction wraps mod
                // 65536 like the real chip, not mod 2^32 (which would
                // produce a length that reads far past the sample buffer and
                // goes silent for the rest of the note).
                let delta = sext24(b1, b2, b3);
                self.loop_start = self.loop_start.wrapping_add_signed(delta);
                self.loop_len = self.loop_len.wrapping_sub_signed(delta >> 1) & 0xFFFF;
                self.loop_active = true;
                true
            }
            0x19 => {
                // <Set one shot sample>
                self.sample_start = 0;
                self.sample_len = 0;
                self.loop_start = 0;
                self.loop_len = 0;
                true
            }
            0x1A => {
                // <Wait on DMA>*
                paula.reset_loop_completions(voice);
                self.wait = Wait::DmaCompletions(word23 as u32);
                false
            }
            0x1C => {
                // <Splitkey>
                if self.note < b1 as i32 {
                    self.step = word23;
                }
                true
            }
            0x1D => {
                // <Splitvol>
                if self.volume < b1 {
                    self.step = word23;
                }
                true
            }
            0x1E => {
                // <AddVol+Note>*
                self.volume = (self.volume as i16 + b3 as i8 as i16).clamp(0, 64) as u8;
                let note = self.note + b1 as i8 as i32 + self.transpose as i32;
                self.period = note_period(note, self.detune);
                self.wait = Wait::Jiffies(0);
                false
            }
            0x1F => {
                // <SetPrevNote>*
                let note = self.last_note + b1 as i8 as i32 + self.transpose as i32;
                self.period = note_period(note, (word23 as i16).saturating_add(self.detune));
                self.wait = Wait::Jiffies(0);
                false
            }
            0x20 => {
                // <Signal>
                self.signals[(b1 & 3) as usize] = word23;
                true
            }
            0x21 => {
                // <Play macro>
                emit(MacroEvent::PlayMacro {
                    channel: b2 & 0x0F,
                    macro_number: b1,
                    detune: b3 as i8,
                });
                true
            }
            // $1B <Random play>: no operand layout or effect stated at all
            // ([S1]: a bare "?"). $22-$29: real-time sample manipulation,
            // "due to lack of research undocumented" ([S1]). Both recorded,
            // never guessed -- docs/opcodes.md Unresolved section.
            other => {
                unsupported.count(other);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module::Module;

    /// Builds a fixed-layout `mdat` whose macro-pointer table (`$600`)
    /// points at each of `macros` in turn -- mirrors `sequencer.rs`'s
    /// `pattern_module` test helper, but for macro data.
    fn macro_module(macros: &[&[[u8; 4]]]) -> Vec<u8> {
        let mut mdat = vec![0u8; 0x900];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let mut offset = 0x900u32;
        for (i, entries) in macros.iter().enumerate() {
            let slot = 0x600 + i * 4;
            mdat[slot..slot + 4].copy_from_slice(&offset.to_be_bytes());
            offset += (entries.len() * 4) as u32;
        }
        for entries in macros {
            for entry in *entries {
                mdat.extend_from_slice(entry);
            }
        }
        mdat
    }

    fn tick(mac: &mut MacroInterpreter, module: &Module, paula: &mut Paula) {
        let mut unsupported = UnsupportedOps::default();
        mac.tick(module, paula, 0, &mut unsupported, |_| {})
            .expect("stays in range");
    }

    fn run(mac: &mut MacroInterpreter, module: &Module, paula: &mut Paula, jiffies: u32) {
        for _ in 0..jiffies {
            tick(mac, module, paula);
        }
    }

    fn stub_module() -> Vec<u8> {
        macro_module(&[&[[0x07, 0, 0, 0]]])
    }

    // -- note_period --

    #[test]
    fn middle_c_matches_the_worked_example() {
        assert_eq!(note_period(0x18, 0), 424);
    }

    #[test]
    fn one_octave_up_halves_the_period() {
        assert_eq!(note_period(0x24, 0), 212);
    }

    #[test]
    fn finetune_plus_50_percent_matches_the_worked_example() {
        assert_eq!(note_period(0x18, 0x0080), 283);
    }

    #[test]
    fn finetune_minus_50_percent_lowers_pitch() {
        assert_eq!(note_period(0x18, -128), 848);
    }

    #[test]
    fn transpose_is_note_index_addition_before_lookup() {
        assert_eq!(note_period(0x18 + 12, 0), note_period(0x24, 0));
    }

    /// Sweeps the whole `$00`-`$3F` note range against an *independently
    /// derived* expectation: instead of the implementation's closed-form
    /// `powf`, walk the equal-temperament semitone ratio 2^(1/12) up or down
    /// from the editor-validated anchor (`$18` = 8363 Hz, see
    /// `MIDDLE_C_NOTE`'s doc comment). A systematic error
    /// in the closed form would not reproduce itself here. Tolerance +/-1 is
    /// exactly the rounding freedom `docs/playback-model.md` §4 leaves open
    /// (round-half-to-even vs. truncate, "pick one and keep it consistent").
    #[test]
    fn note_period_matches_an_independently_walked_semitone_ratio() {
        /// 2^(1/12), the equal-temperament semitone ratio, as a literal --
        /// deliberately not computed with `powf`.
        const SEMITONE: f64 = 1.059_463_094_359_295_3;

        for note in 0..64i32 {
            let steps = note - MIDDLE_C_NOTE;
            let mut freq = MIDDLE_C_HZ;
            for _ in 0..steps.abs() {
                if steps > 0 {
                    freq *= SEMITONE;
                } else {
                    freq /= SEMITONE;
                }
            }
            let expected = (PAULA_CLOCK_HZ / freq).round() as i32;
            let actual = note_period(note, 0) as i32;
            assert!(
                (actual - expected).abs() <= 1,
                "note {note:#04X}: period {actual}, independently derived {expected}"
            );
        }
    }

    /// The structural invariant [S1] states outright for one pair (`$2A` is
    /// "exactly half of the `$1E` period"), checked across the whole range
    /// rather than at that single documented point.
    #[test]
    fn note_period_halves_every_octave_across_the_whole_range() {
        for note in 0..52i32 {
            let low = note_period(note, 0) as i32;
            let high = note_period(note + 12, 0) as i32;
            assert!(
                (low - 2 * high).abs() <= 1,
                "note {note:#04X}: period {low}, an octave up {high} (expected ~{})",
                low / 2
            );
        }
    }

    /// The pattern record's 8-bit `dd` and the macro opcodes' 16-bit `bbbb`
    /// are one convention at two widths (`docs/playback-model.md` §4): a
    /// sign-extended 8-bit detune must land on the same multiplier as the
    /// numerically equal 16-bit finetune, including at both extremes.
    #[test]
    fn eight_bit_detune_sign_extends_onto_the_sixteen_bit_scale() {
        assert_eq!(
            note_period(0x1E, i8::MIN as i16),
            note_period(0x1E, 0xFF80_u16 as i16),
            "8-bit -128 must be the 16-bit $FF80 worked point (50%)"
        );
        assert_eq!(
            note_period(0x1E, i8::MAX as i16),
            note_period(0x1E, 0x007F_u16 as i16),
            "8-bit +127 must be the 16-bit $007F (~+49.6%)"
        );
    }

    // -- $00-$03: DMA, sample region --

    #[test]
    fn dma_set_begin_set_len_reach_paula() {
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x00, 0x10],
            [0x03, 0x00, 0x00, 0x05],
            [0x01, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x1E, 15, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.start, 0x10);
        assert_eq!(v.len, 5);
        assert!(v.dma_on);
        assert!(mac.is_stopped()); // the program's own $07 stops it
    }

    #[test]
    fn set_begin_is_absolute_not_cumulative() {
        // `docs/playback-model.md` §2's own table already states `$02
        // SetBegin` targets an "absolute smpl offset" -- unlike `$18
        // Sampleloop`'s explicitly-documented non-idempotent, additive
        // delta (§7 gotchas). A second `$02` within the same trigger (a
        // common corpus idiom: an attack region followed by an unrelated
        // sustain/loop region, e.g. `turrican intro` macro 28, or several
        // one-shot samples played back to back, e.g. `turrican 2
        // level 1-desert` macro 33) must replace the pointer, not add onto
        // it.
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x00, 0x64], // SetBegin +100
            [0x02, 0x00, 0x00, 0xC8], // SetBegin +200 -- absolute, not +300
            [0x03, 0x00, 0x00, 0x05],
            [0x01, 0x00, 0x00, 0x00],
            [0x07, 0x00, 0x00, 0x00],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.start, 200);
    }

    #[test]
    fn dma_off_reset_with_nonzero_aa_does_not_suspend() {
        let mdat = macro_module(&[&[
            [0x01, 0, 0, 0],
            [0x00, 0x01, 0, 0], // aa != 0 -> immediate, no suspend
            [0x0E, 0x20, 0, 0], // SetVolume 0x20, runs the SAME jiffy
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        let v = paula.voice(0);
        assert!(!v.dma_on); // $00 turned it back off
        assert_eq!(v.volume, 0x20); // and the next op still ran this jiffy
    }

    #[test]
    fn dma_off_reset_with_zero_aa_suspends_one_jiffy() {
        let mdat = macro_module(&[&[
            [0x00, 0x00, 0, 0], // aa == 0 -> suspend 1 jiffy
            [0x0E, 0x20, 0, 0],
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 0); // not yet
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 0x20); // runs next jiffy
    }

    // -- $04 Wait --

    #[test]
    fn wait_holds_for_exactly_aaaa_jiffies() {
        let mdat = macro_module(&[&[
            [0x04, 0x00, 0x00, 0x03],
            [0x0E, 0x28, 0x00, 0x00],
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        for _ in 0..3 {
            tick(&mut mac, &module, &mut paula);
            assert_eq!(paula.voice(0).volume, 0, "still waiting");
        }
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 0x28);
    }

    #[test]
    fn wait_with_zero_jiffies_still_suspends_one_jiffy() {
        // `$04` is asterisked (`docs/opcodes.md` §3 intro): it can suspend the
        // macro program, same convention as `$00`'s own `aa == 0` case
        // (`dma_off_reset_with_zero_aa_suspends_one_jiffy` above). `aaaa == 0`
        // must still yield to the next jiffy, not skip suspension outright --
        // otherwise a `$05 Loop` with no other suspending step in its body
        // (e.g. `turrican intro` macro 10) spins the whole loop instantly
        // every tick instead of pacing one iteration per real jiffy.
        let mdat = macro_module(&[&[
            [0x04, 0x00, 0x00, 0x00],
            [0x0E, 0x20, 0x00, 0x00],
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 0, "not yet -- $04 still suspends one jiffy");
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 0x20, "runs next jiffy");
    }

    // -- $05/$10 Loop --

    #[test]
    fn loop_repeats_the_block_aa_times_then_falls_through() {
        let mdat = macro_module(&[&[
            [0x0D, 0, 0, 1],
            [0x05, 0x02, 0x00, 0x00], // loop to step 0, 2 extra passes
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 3); // 1 initial pass + 2 repeats
    }

    #[test]
    fn unconditional_loop_does_not_poison_a_nested_counted_loops_repeat_state() {
        // `turrican intro` macro 0x1c (28) nests a counted `$05 <Loop>` (aa=3,
        // say) inside an outer *unconditional* one (aa=0, "jump back forever" --
        // this crate's own reading of aa=0, matching the corpus). Both share
        // one `self.repeat` field. The `times == 0` arm used to call
        // `self.repeat.get_or_insert(0)` same as the counted arm, which -- on
        // a fresh `None` -- inserts `Some(0)` and leaves it there after
        // jumping. The next time a *counted* loop instruction is reached, it
        // finds that stale `Some(0)` instead of `None`, reads `left == 0`, and
        // treats itself as already exhausted after a single pass instead of
        // its own `aa` repeats -- confirmed live via `tfmx-cli`'s `render`
        // (instrumented) on `turrican intro`: the inner loop ran its full 6
        // passes once, then only 1 pass on every subsequent cycle through the
        // outer loop, producing a one-sided pointer drift instead of the
        // intended symmetric wobble (`docs/macro-playback-fidelity.md` §8).
        let mdat = macro_module(&[&[
            [0x0D, 0, 0, 1],          // step 0: AddVolume +1
            [0x05, 0x03, 0x00, 0x00], // step 1: counted loop, aa=3 -> 4 passes
            [0x04, 0, 0, 0],          // step 2: Wait 1 jiffy
            [0x05, 0x00, 0x00, 0x00], // step 3: unconditional loop back to 0
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 4, "first pass through the counted loop");
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(
            paula.voice(0).volume,
            8,
            "second pass, after the outer unconditional loop, must run the \
             same 4 counted-loop repeats as the first -- not just 1"
        );
    }

    #[test]
    fn loop_key_up_breaks_out_early_once_signaled() {
        let mdat = macro_module(&[&[
            [0x0D, 0, 0, 1],
            [0x10, 0x00, 0x00, 0x00], // loop-key-up, indefinite unless key_up
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        mac.signal_key_up();
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 1); // one pass, then breaks out
    }

    // -- $06 Cont, $15/$16 GoSub/Return --

    #[test]
    fn gosub_skips_straight_to_the_target_step_and_returns() {
        let mdat = macro_module(&[
            &[
                [0x0D, 0, 0, 1],          // marker A
                [0x15, 0x01, 0x00, 0x01], // gosub macro 1, step 1
                [0x0D, 0, 0, 2],          // marker C (after return)
                [0x07, 0, 0, 0],
            ],
            &[
                [0x0D, 0, 0, 100], // step 0, never reached (target is step 1)
                [0x16, 0, 0, 0],   // return
            ],
        ]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 3); // 1 + 2, marker B's 100 skipped
    }

    #[test]
    fn cont_jumps_into_another_macro_without_saving() {
        let mdat = macro_module(&[
            &[[0x06, 0x01, 0x00, 0x00]],
            &[[0x0E, 0x2A, 0x00, 0x00], [0x07, 0, 0, 0]],
        ]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 0x2A);
        assert_eq!(mac.macro_number(), 1);
    }

    // -- $07 STOP --

    #[test]
    fn stop_parks_the_program_until_retriggered() {
        let mdat = macro_module(&[&[[0x07, 0, 0, 0], [0x0E, 0x10, 0, 0]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 5);
        assert!(mac.is_stopped());
        assert_eq!(paula.voice(0).volume, 0);
        mac.trigger(0, 0, 0, 0);
        assert!(!mac.is_stopped());
    }

    // -- `note_on`: same-macro retrigger while already running --

    /// The `turrican intro` bug (`docs/status.md`, step 11.5's `no-retrigger`
    /// finding): a fast note run retriggers the *same* macro on the *same*
    /// voice every jiffy. Every macro in that corpus module opens with `$00
    /// aa=0` (mandatory 1-jiffy pause) and ends its note-setting opcode
    /// (`$08`/`$09`) with another 1-jiffy suspend, so `$01 DMAon` needs two
    /// clear jiffies after a `trigger()` to ever run. A full `trigger()` on
    /// every repeat note resets `step` back to 0 each time, so `$01` is
    /// never reached -- confirmed silent against this crate's own render,
    /// where an A/B against `uade123` shows continuous audible output for
    /// this same passage.
    #[test]
    fn note_on_retriggering_the_still_running_macro_does_not_reset_dma() {
        let mdat = macro_module(&[&[
            [0x00, 0, 0, 0],          // $00 aa=0: mandatory 1-jiffy pause
            [0x02, 0x00, 0x00, 0x10], // SetBegin
            [0x03, 0x00, 0x00, 0x05], // SetLen
            [0x08, 0x00, 0x00, 0x00], // AddNote: suspends this jiffy too
            [0x01, 0, 0, 0],          // DMAon
            [0x14, 0, 0, 0],          // Wait key up, indefinitely
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        let mut paula = Paula::new(100);

        mac.trigger(0, 21, 8, 0);
        run(&mut mac, &module, &mut paula, 3); // clears both suspends
        assert!(paula.voice(0).dma_on, "should have reached $01 by jiffy 3");

        // A same-macro retrigger while the program is still alive (parked
        // in `$14`'s indefinite wait, not yet `$07`-stopped) must not wipe
        // out what `$01` already latched.
        mac.note_on(0, 23, 8, 0, 0);
        run(&mut mac, &module, &mut paula, 1);
        assert!(
            paula.voice(0).dma_on,
            "a same-macro retrigger should not reset dma_on"
        );
    }

    #[test]
    fn note_on_retriggering_through_a_cont_indirection_does_not_reset_dma() {
        // `turrican intro`'s voice-3 percussion instrument: the *public*
        // macro number a pattern's Note event names (24 in the real
        // corpus) opens with `$1C <Splitkey>`/`$06 <Cont>` into a
        // *different* macro number that actually does the sample setup and
        // `$01 DMAon`. Both `$1C` and `$06` run within the *same* jiffy as
        // the trigger (neither suspends), so by the time the next Note
        // event for this same instrument arrives, `self.macro_number` has
        // already moved on to the target macro -- `note_on`'s
        // `macro_number == self.macro_number` check compares the pattern's
        // instrument number against the wrong thing and always takes the
        // full-reset branch, so a fast repeat (one retrigger per jiffy)
        // never survives past the target macro's own `$00 aa=0` pause to
        // reach its `$01 DMAon`, exactly like the already-fixed same-macro
        // case but for any instrument that dispatches through `$06`.
        let mdat = macro_module(&[
            &[[0x06, 0x01, 0x00, 0x00]], // macro 0: Cont into macro 1, step 0
            &[
                [0x00, 0, 0, 0],          // $00 aa=0: mandatory 1-jiffy pause
                [0x02, 0x00, 0x00, 0x10], // SetBegin
                [0x03, 0x00, 0x00, 0x05], // SetLen
                [0x08, 0x00, 0x00, 0x00], // AddNote: suspends this jiffy too
                [0x01, 0, 0, 0],          // DMAon
                [0x14, 0, 0, 0],          // Wait key up, indefinitely
            ],
        ]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        let mut paula = Paula::new(100);

        // A fast note run: the pattern retriggers instrument 0 every single
        // jiffy, never giving the target macro two clear jiffies in a row.
        for _ in 0..5 {
            mac.note_on(0, 21, 8, 0, 0);
            run(&mut mac, &module, &mut paula, 1);
        }
        assert!(
            paula.voice(0).dma_on,
            "a per-jiffy retrigger of the same instrument must eventually \
             reach $01 DMAon even when the instrument's own macro jumps to \
             a different macro number via $06 Cont"
        );
    }

    #[test]
    fn note_on_with_a_different_macro_still_does_a_full_reset() {
        let mdat = macro_module(&[
            &[
                [0x02, 0x00, 0x00, 0x10],
                [0x03, 0x00, 0x00, 0x05],
                [0x01, 0, 0, 0],
                [0x14, 0, 0, 0],
            ],
            &[[0x07, 0, 0, 0]],
        ]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        let mut paula = Paula::new(100);

        mac.trigger(0, 21, 8, 0);
        run(&mut mac, &module, &mut paula, 1);
        assert!(paula.voice(0).dma_on);

        mac.note_on(1, 21, 8, 0, 0);
        run(&mut mac, &module, &mut paula, 1);
        assert!(
            !paula.voice(0).dma_on,
            "a different macro number must still fully retrigger"
        );
    }

    #[test]
    fn note_on_after_stop_does_a_full_reset_even_for_the_same_macro() {
        let mdat = macro_module(&[&[
            [0x07, 0, 0, 0],
            [0x02, 0x00, 0x00, 0x10],
            [0x03, 0x00, 0x00, 0x05],
            [0x01, 0, 0, 0],
            [0x14, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        let mut paula = Paula::new(100);

        mac.trigger(0, 21, 8, 0);
        run(&mut mac, &module, &mut paula, 1);
        assert!(mac.is_stopped());

        // Retriggering macro 0 again after it stopped must restart at step
        // 0, not resume from where $07 parked it.
        mac.note_on(0, 21, 8, 0, 0);
        run(&mut mac, &module, &mut paula, 1);
        assert!(mac.is_stopped(), "step 0's own $07 should stop it again");
    }

    // -- $08/$09/$1F: note, transpose, finetune --

    #[test]
    fn add_note_transposes_and_ends_processing_this_jiffy() {
        let mdat = macro_module(&[&[
            [0x08, 0x06, 0x00, 0x00], // transpose +6
            [0x0E, 0x01, 0x00, 0x00], // would run same jiffy if not suspended
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x12, 0, 0); // note $12 + 6 = $18
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, 424);
        assert_eq!(paula.voice(0).volume, 0); // $0E did not run this jiffy
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 1); // runs the next jiffy
    }

    #[test]
    fn set_note_uses_the_operand_directly_not_the_triggering_note() {
        let mdat = macro_module(&[&[[0x09, 0x18, 0x00, 0x00]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x00, 0, 0); // triggering note is irrelevant to $09
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, 424);
    }

    /// The pattern note record's `dd` detune (`NoteTiming::Detune`, decoded
    /// in `sequencer.rs`) must reach the note's period through the same
    /// finetune channel `$21`'s detune uses -- `docs/playback-model.md` §4
    /// states `dd` is a finetune whenever the note byte is below `$80`.
    #[test]
    fn note_on_detune_reaches_the_notes_period() {
        let mdat = macro_module(&[&[[0x09, 0x1E, 0x00, 0x00]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.note_on(0, 0x1E, 0, 0, 0x40);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, note_period(0x1E, 0x40));
        assert_ne!(paula.voice(0).period, 424, "the detune must not be dropped");
    }

    /// ...and a fresh note replaces a leftover `$21` detune rather than
    /// inheriting it.
    #[test]
    fn note_on_replaces_a_stale_play_macro_detune() {
        let mdat = macro_module(&[&[[0x09, 0x18, 0x00, 0x00]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x18, 0, 0);
        mac.play_macro(0, 0x40);
        mac.note_on(0, 0x18, 0, 0, 0);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, 424);
    }

    /// `word23 + self.detune` combines a 16-bit macro finetune with an 8-bit
    /// `$21` detune. The sum must saturate, not overflow: debug builds panic
    /// on `i16` overflow and `tfmx/tests/mutation_robustness.rs` requires
    /// corrupted module data never to panic.
    #[test]
    fn macro_finetune_plus_detune_saturates_instead_of_overflowing() {
        let mdat = macro_module(&[&[[0x09, 0x1E, 0x7F, 0xFF], [0x07, 0, 0, 0]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x1E, 0, 0);
        mac.play_macro(0, i8::MAX);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, note_period(0x1E, i16::MAX));
    }

    #[test]
    fn set_prev_note_uses_the_note_before_the_current_trigger() {
        let mdat = macro_module(&[&[[0x1F, 0x00, 0x00, 0x00]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x18, 0, 0); // last_note becomes $18 on the next trigger
        mac.trigger(0, 0x00, 0, 0); // current note $00, last_note $18
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, 424);
    }

    // -- $0A Reset --

    #[test]
    fn reset_stops_vibrato_portamento_and_envelope() {
        let mdat = macro_module(&[&[[0x0A, 0, 0, 0], [0x07, 0, 0, 0]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        mac.period = 424;
        mac.start_vibrato(4, 10);
        mac.start_portamento(1, 10);
        mac.start_envelope(5, 1, 40);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert!(mac.vibrato.is_none());
        assert!(mac.portamento.is_none());
        assert!(mac.envelope.is_none());
        let period_after = paula.voice(0).period;
        // The program has stopped ($07); nothing is modulating the period
        // anymore, so it must stay put on the next jiffy too.
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, period_after);
    }

    // -- $0B/$0C/$0F effects --

    #[test]
    fn portamento_matches_the_worked_example_every_jiffy() {
        let mdat = stub_module();
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        mac.period = 424;
        mac.start_portamento(1, 10);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, 440);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, 457);
    }

    #[test]
    fn vibrato_is_a_bipolar_triangle_returning_to_base_each_cycle() {
        let mdat = stub_module();
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        mac.period = 424;
        mac.start_vibrato(4, 10);
        let mut paula = Paula::new(100);
        let mut periods = Vec::new();
        for _ in 0..9 {
            tick(&mut mac, &module, &mut paula);
            periods.push(paula.voice(0).period);
        }
        assert_eq!(periods, vec![424, 434, 444, 434, 424, 414, 404, 414, 424]);
    }

    #[test]
    fn envelope_matches_the_worked_example_and_clamps_at_target() {
        let mdat = stub_module();
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        mac.volume = 16;
        mac.start_envelope(5, 3, 40);
        let mut paula = Paula::new(100);
        let mut volumes = Vec::new();
        for _ in 0..18 {
            tick(&mut mac, &module, &mut paula);
            volumes.push(paula.voice(0).volume);
        }
        assert!(volumes.iter().all(|&v| v <= 40));
        assert!(volumes.contains(&21));
        assert!(volumes.contains(&26));
        assert_eq!(*volumes.last().unwrap(), 40);
        assert_eq!(volumes[volumes.len() - 2], 40); // stopped advancing
    }

    // -- $12/$18/$19: sample length, loop region, one-shot --

    #[test]
    fn sampleloop_compounds_on_repeated_calls() {
        // Deltas are even (word-aligned, as real corpus data is) so halving
        // is exact -- see the `$18` arm's own comment for why the delta
        // (bytes, same units as `$02 SetBegin`) must be halved before it hits
        // `loop_len` (words, Paula's length register, `$03 SetLen`'s units).
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x00, 0x64], // SetBegin +100
            [0x03, 0x00, 0x00, 0x0A], // SetLen 10
            [0x18, 0x00, 0x00, 0x04], // Sampleloop +4 bytes / -2 words
            [0x18, 0x00, 0x00, 0x04], // again -- compounds, not idempotent
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.start, 100); // attack region untouched by $18
        assert_eq!(v.len, 10);
        assert_eq!(v.loop_start, 108); // two +4-byte calls
        assert_eq!(v.loop_len, 6); // two -2-word calls
    }

    #[test]
    fn sampleloop_preserves_the_sample_end_point() {
        // `docs/opcodes.md` §4: "$18 ... moves the loop start forward (or
        // back) by aaaaaa bytes while shrinking (or growing) the remaining
        // sample length by the same amount, so the sample's end point is
        // unchanged." That's only true in *byte* terms if the byte-valued
        // delta is halved before it's subtracted from the word-valued
        // `loop_len` -- pin the invariant directly rather than a magic
        // number.
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x00, 0x64], // SetBegin +100
            [0x03, 0x00, 0x00, 0x0A], // SetLen 10 words -- end = 100 + 20 = 120
            [0x18, 0x00, 0x00, 0x08], // Sampleloop +8 bytes
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.loop_start as u64 + v.loop_len as u64 * 2, 120);
    }

    #[test]
    fn sampleloop_underflow_wraps_at_16_bits_like_real_paula_hardware() {
        // Paula's length register is 16-bit hardware (`docs/format.md` §8);
        // a real chip wraps an underflowing subtraction mod 65536, not mod
        // 2^32. Wrapping in 32-bit space instead produces a length near
        // `u32::MAX`, which makes `Voice::next_sample` read far past the
        // sample buffer forever -- silently returning 0 (silence) for the
        // rest of the note instead of the wrapped-but-audible waveform real
        // hardware would produce.
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x00, 0x64], // SetBegin +100
            [0x03, 0x00, 0x00, 0x0A], // SetLen 10 -- loop_len starts at 10
            [0x18, 0x00, 0x00, 0x1E], // Sampleloop +30 bytes / -15 words: underflows
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.loop_len, 65531); // (10i32 - 15) as u16 -- not ~u32::MAX
    }

    #[test]
    fn sampleloop_keeps_turrican_intro_macro_28_in_bounds() {
        // `turrican intro` pattern 0x52/macro 0x1c on voice 0
        // (`docs/macro-playback-fidelity.md` §5): before the `$18` fix,
        // `loop_len` computed as 1024 - 1792 (masked mod 65536) = 64768
        // words = 129536 bytes, reaching far past the 45828-byte
        // `smpl.turrican intro` file and reading as silence for most of the
        // note. Halving the delta first keeps the sample's end point fixed
        // and lands well within bounds. The macro's second `$02 SetBegin`
        // is absolute (`set_begin_is_absolute_not_cumulative`), so it
        // replaces the first rather than adding onto it.
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x1C, 0x14], // SetBegin +0x1C14 = 7188 -- overwritten below
            [0x02, 0x00, 0x78, 0x04], // SetBegin +0x7804 = 30724, absolute
            [0x03, 0x00, 0x04, 0x00], // SetLen 0x0400 = 1024 words
            [0x18, 0x00, 0x07, 0x00], // Sampleloop +0x0700 = 1792 bytes
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.loop_start, 32516); // 30724 + 1792
        assert_eq!(v.loop_len, 128); // unaffected by the SetBegin fix
        let loop_end_bytes = v.loop_start as u64 + v.loop_len as u64 * 2;
        assert!(
            loop_end_bytes < 45828,
            "loop region must stay within smpl.turrican intro's 45828 bytes, got end={loop_end_bytes}"
        );
    }

    #[test]
    fn add_begin_after_sampleloop_wobbles_the_loop_pointer_not_the_stale_attack_one() {
        // `$11 <AddBegin>`'s one-shot form (`aa=0`) nudges "the sample
        // pointer" (`docs/opcodes.md` §3) -- but once `$18 <Sampleloop>` has
        // already handed the voice off to loop playback, the only pointer
        // still being read is `loop_start`/`loop_len`; `sample_start`/
        // `sample_len` are a stale pre-loop snapshot. Before this fix, `$11`
        // always wobbled `sample_start`, and `render` pushed it through
        // `Paula::set_sample_region` every jiffy regardless of loop phase --
        // silently undoing `Voice::next_sample`'s one-shot->loop handoff and
        // dragging the voice's real read pointer to an address the loop math
        // never accounted for (measured on `turrican intro` pattern
        // 0x52/macro 0x1c via `tfmx-cli lint`'s `sample-region-out-of-bounds`
        // finding, `docs/macro-playback-fidelity.md` §8).
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x00, 0x64], // SetBegin +100
            [0x03, 0x00, 0x00, 0x0A], // SetLen 10 words
            [0x01, 0x00, 0x00, 0x00], // DMAon
            [0x18, 0x00, 0x00, 0x08], // Sampleloop +8 bytes -> loop_start=108, loop_len=6
            [0x11, 0x00, 0x01, 0x00], // AddBegin one-shot +256
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.loop_start, 108 + 256); // the wobble lands on the live loop pointer
        assert_eq!(v.start, 100); // attack region untouched, not re-dragged past the loop
    }

    #[test]
    fn set_one_shot_sample_silences_the_voice() {
        let mdat = macro_module(&[&[
            [0x02, 0x00, 0x00, 0x64],
            [0x03, 0x00, 0x00, 0x0A],
            [0x18, 0x00, 0x00, 0x05],
            [0x19, 0x00, 0x00, 0x00],
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        let v = paula.voice(0);
        assert_eq!(v.start, 0);
        assert_eq!(v.len, 0);
        assert_eq!(v.loop_start, 0);
        assert_eq!(v.loop_len, 0);
    }

    // -- $14 Wait key up --

    #[test]
    fn wait_key_up_expires_after_aa_jiffies_without_a_signal() {
        let mdat = macro_module(&[&[
            [0x14, 0x00, 0x00, 0x05],
            [0x0E, 0x20, 0x00, 0x00],
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        for _ in 0..5 {
            tick(&mut mac, &module, &mut paula);
            assert_eq!(paula.voice(0).volume, 0);
        }
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 0x20);
    }

    #[test]
    fn wait_key_up_wakes_immediately_on_signal() {
        let mdat = macro_module(&[&[
            [0x14, 0x00, 0x00, 0x00], // aa=0: indefinite without a signal
            [0x0E, 0x20, 0x00, 0x00],
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula); // enters the indefinite wait
        for _ in 0..50 {
            tick(&mut mac, &module, &mut paula);
            assert_eq!(paula.voice(0).volume, 0, "never wakes on its own");
        }
        mac.signal_key_up();
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).volume, 0x20);
    }

    // -- $1A Wait on DMA --

    #[test]
    fn wait_on_dma_opcode_resets_completions_and_suspends() {
        let mdat = macro_module(&[&[[0x1A, 0x00, 0x00, 0x03], [0x0E, 0x20, 0, 0]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(mac.wait, Wait::DmaCompletions(3));
        assert_eq!(paula.loop_completions(0), 0);
    }

    #[test]
    fn take_turn_resumes_once_loop_completions_reach_target() {
        let mut mac = MacroInterpreter::new();
        mac.wait = Wait::DmaCompletions(2);
        let mut paula = Paula::new(100);
        assert!(!mac.take_turn(&mut paula, 0));

        paula.set_dma(0, true);
        paula.set_period(0, 1); // very high frequency
        paula.set_sample_region(0, 0, 1); // len 1 word = 2 samples
        paula.set_loop_region(0, 0, 1); // reload the same region on wrap
        let smpl = [0i8; 4];
        let mut out = [0i16; 8];
        paula.render(&smpl, 44100, &mut out);
        assert!(paula.loop_completions(0) >= 2);

        assert!(mac.take_turn(&mut paula, 0));
        assert_eq!(mac.wait, Wait::Ready);
    }

    // -- $1C/$1D Splitkey/Splitvol --

    #[test]
    fn splitkey_jumps_only_when_note_is_below_the_threshold() {
        let mdat = macro_module(&[&[
            [0x1C, 0x20, 0x00, 0x03],
            [0x0D, 0, 0, 1], // not-taken marker
            [0x07, 0, 0, 0],
            [0x0D, 0, 0, 10], // taken marker
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");

        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x18, 0, 0); // $18 < $20 -> taken
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 10);

        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x28, 0, 0); // $28 >= $20 -> not taken
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 1);
    }

    #[test]
    fn splitvol_jumps_only_when_volume_is_below_the_threshold() {
        let mdat = macro_module(&[&[
            [0x1D, 0x20, 0x00, 0x03],
            [0x0D, 0, 0, 1],
            [0x07, 0, 0, 0],
            [0x0D, 0, 0, 10],
            [0x07, 0, 0, 0],
        ]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");

        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 5, 0); // volume 5*3=15 < $20 -> taken
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 25); // 15 + 10

        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 15, 0); // volume 15*3=45 >= $20 -> not taken
        let mut paula = Paula::new(100);
        run(&mut mac, &module, &mut paula, 1);
        assert_eq!(paula.voice(0).volume, 46); // 45 + 1
    }

    // -- $1E AddVol+Note --

    #[test]
    fn add_vol_plus_note_does_both_in_one_opcode() {
        let mdat = macro_module(&[&[[0x1E, 0x06, 0xFE, 0x0A]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0x12, 0, 0); // note $12 + 6 = $18
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(paula.voice(0).period, 424);
        assert_eq!(paula.voice(0).volume, 10);
    }

    // -- $20 Signal --

    #[test]
    fn signal_stores_into_the_selected_register() {
        let mdat = macro_module(&[&[[0x20, 0x05, 0x12, 0x34], [0x07, 0, 0, 0]]]); // aa&3 = 1
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        tick(&mut mac, &module, &mut paula);
        assert_eq!(mac.signals[1], 0x1234);
        assert_eq!(mac.signals[0], 0);
    }

    // -- $21 Play macro --

    #[test]
    fn play_macro_emits_a_cross_voice_event() {
        let mdat = macro_module(&[&[[0x21, 0x05, 0x02, 0x10], [0x07, 0, 0, 0]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        let mut unsupported = UnsupportedOps::default();
        let mut events = Vec::new();
        mac.tick(&module, &mut paula, 0, &mut unsupported, |e| events.push(e))
            .unwrap();
        assert_eq!(
            events,
            vec![MacroEvent::PlayMacro {
                channel: 2,
                macro_number: 5,
                detune: 0x10,
            }]
        );
    }

    // -- Unknown opcodes: $1B, $22-$29 --

    #[test]
    fn unknown_opcodes_are_recorded_never_guessed() {
        let mdat = macro_module(&[&[[0x24, 0, 0, 0], [0x1B, 0, 0, 0], [0x07, 0, 0, 0]]]);
        let module = Module::parse(&mdat, &[]).expect("valid header parses");
        let mut mac = MacroInterpreter::new();
        mac.trigger(0, 0, 0, 0);
        let mut paula = Paula::new(100);
        let mut unsupported = UnsupportedOps::default();
        mac.tick(&module, &mut paula, 0, &mut unsupported, |_| {})
            .unwrap();
        assert_eq!(unsupported.get(0x24), 1);
        assert_eq!(unsupported.get(0x1B), 1);
        assert_eq!(unsupported.get(0x25), 0);
        assert!(mac.is_stopped()); // execution kept going past the unknowns
    }
}
