//! Static analysis over `tfmx::Module`: resolves a song to its reachable
//! patterns, macros and sample regions without running the player.
//!
//! See `docs/m5-plan.md` Phase 5.2 for the design brief this implements.

mod disasm;
mod view;
mod walker;
mod zones;
pub use disasm::{DisasmLine, disassemble_macro, disassemble_pattern};
pub use view::{
    LineCommandView, SongView, StepView, TrackSlotView, TrackstepMap, TrackstepStep,
    WaveformRegion, WaveformView, build_song_view,
};
pub use walker::{Edge, SampleRegion, Span, SpanKind, WalkResult, walk_song};
pub use zones::{
    Envelope, MacroVolume, NOTE_MAX, VOLUME_MAX, Zone, ZoneExit, ZoneTable, resolve_zones,
};
