//! Static analysis over `tfmx::Module`: resolves a song to its reachable
//! patterns, macros and sample regions without running the player.
//!
//! See `docs/m5-plan.md` Phase 5.2 for the design brief this implements.

mod walker;
mod zones;
pub use walker::{SampleRegion, Span, SpanKind, WalkResult, walk_song};
pub use zones::{
    Envelope, MacroVolume, NOTE_MAX, VOLUME_MAX, Zone, ZoneExit, ZoneTable, resolve_zones,
};
