//! Decoder and software mixer for Amiga TFMX music modules.
//!
//! See `docs/` in the repository for the format documentation this crate is
//! implemented from: `format.md` (data model), `opcodes.md` (command reference),
//! `playback-model.md` (how sound is produced) and `architecture.md` (code shape).

mod module;
mod paula;
mod sequencer;
pub use module::{AccessError, Module, ParseError};
pub use paula::{Paula, Voice};
pub use sequencer::{
    LineCommand, NoteTiming, PatternCommand, PatternEntry, PatternRunner, Sequencer, TickClock,
    TrackSlot, TrackstepLine, tick_fraction,
};
