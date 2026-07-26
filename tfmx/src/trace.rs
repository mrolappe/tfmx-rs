//! [`TraceEvent`]: the observation seam. `Player::render_traced` (step 11.3)
//! emits one of these for every state-machine transition a jiffy produces,
//! alongside the register seam `docs/architecture.md` §2 already documents.
//! `render()` is `render_traced(.., |_| {})` monomorphized away -- the trace
//! seam costs nothing when unused and cannot change what gets rendered.

use crate::paula::Voice;
use crate::sequencer::{PatternEntry, TrackstepLine};

/// One state-machine transition traced during a single jiffy. Variants fire
/// in this order within a jiffy: [`TraceEvent::Jiffy`] first (the tick
/// boundary itself, before the trackstep line is even read), then
/// [`TraceEvent::Trackstep`] once the line is decoded, then one
/// [`TraceEvent::Pattern`] (and, for a note, one [`TraceEvent::Trigger`])
/// per pattern entry executed across all eight tracks, then four
/// [`TraceEvent::Voice`] snapshots at the jiffy's end.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceEvent {
    /// The tick boundary at output sample `frame`, before this jiffy's
    /// trackstep line is processed: `line` is the line about to run,
    /// `tempo` the tempo still in effect, `stopped` whether the player was
    /// already halted entering this jiffy.
    Jiffy {
        frame: u64,
        line: u16,
        tempo: u16,
        stopped: bool,
    },
    /// The trackstep line just decoded and applied by [`crate::Sequencer`].
    Trackstep(TrackstepLine),
    /// One pattern longword executed on `track`'s pattern, fetched from
    /// `pattern` step `step` -- not necessarily the track's *current*
    /// pattern/step after this call, since `$F1`/`$F2`/`$F8` can move the
    /// program counter again within the same jiffy.
    Pattern {
        track: u8,
        pattern: u8,
        step: u16,
        entry: PatternEntry,
    },
    /// A macro program (re)started on `voice` by a pattern note. Emitted
    /// from the same place `voice_of()`'s nibble mask is applied, so a
    /// masking bug shows up here rather than being silently reinterpreted
    /// by a trace consumer that re-derives it.
    Trigger {
        voice: u8,
        macro_number: u8,
        note: u8,
        volume: u8,
        transpose: i8,
    },
    /// `voice`'s landed Paula register state at this jiffy's end -- fires
    /// for all four voices every jiffy; skipping unchanged ones is a trace
    /// consumer's job (step 11.4), not this seam's.
    Voice { voice: u8, state: Voice },
}
