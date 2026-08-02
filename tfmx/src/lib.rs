//! Decoder and software mixer for Amiga TFMX music modules.
//!
//! See `docs/` in the repository for the format documentation this crate is
//! implemented from: `format.md` (data model), `opcodes.md` (command reference),
//! `playback-model.md` (how sound is produced) and `architecture.md` (code shape).

mod macro_interp;
mod module;
mod paula;
mod player;
mod sequencer;
mod trace;
pub use macro_interp::{MacroEvent, MacroInterpreter, UnsupportedOps};
pub use module::{AccessError, Module, ParseError};
pub use paula::{Paula, Voice};
pub use player::{Player, TrackstepGate};
pub use sequencer::{
    LineCommand, NoteTiming, PatternCommand, PatternEntry, PatternRunner, Sequencer, TickClock,
    TrackSlot, TrackstepLine, decode_line, decode_pattern_entry, tick_fraction,
};
pub use trace::TraceEvent;
