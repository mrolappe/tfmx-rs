//! JSON emitters, kept in one place. `dump`'s output rides on
//! `tfmx-analysis`'s own `serde` derives (Phase 5.4); `trace --format
//! json`'s per-event encoder is hand-written because `tfmx`'s core types
//! stay dependency-free and carry no `Serialize` impl.

use std::io::Write;

use serde::Serialize;
use tfmx::TraceEvent;
use tfmx_analysis::{WalkResult, ZoneTable};

use crate::CliError;

#[derive(Serialize)]
struct DumpOutput<'a> {
    song: u8,
    is_7v: bool,
    walk: &'a WalkResult,
    zones: &'a [ZoneTable],
}

pub fn write_dump_json(
    song: u8,
    walk: &WalkResult,
    zones: &[ZoneTable],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let output = DumpOutput {
        song,
        is_7v: walk.is_7v(),
        walk,
        zones,
    };
    serde_json::to_writer_pretty(&mut *out, &output).map_err(CliError::Json)?;
    writeln!(out)?;
    Ok(())
}

/// One [`TraceEvent`] as one JSON object -- mirrors `write_text_event`'s
/// one-line-per-event shape (ndjson), so the two formats stay diffable
/// against each other.
pub fn write_json_event(e: &TraceEvent, out: &mut impl Write) -> std::io::Result<()> {
    let value = match e {
        TraceEvent::Jiffy {
            frame,
            line,
            tempo,
            stopped,
        } => serde_json::json!({
            "type": "jiffy", "frame": frame, "line": line, "tempo": tempo, "stopped": stopped
        }),
        TraceEvent::Trackstep(line) => serde_json::json!({
            "type": "trackstep", "line": format!("{line:?}")
        }),
        TraceEvent::Pattern {
            track,
            pattern,
            step,
            entry,
        } => serde_json::json!({
            "type": "pattern", "track": track, "pattern": pattern, "step": step,
            "entry": format!("{entry:?}")
        }),
        TraceEvent::Trigger {
            voice,
            macro_number,
            note,
            volume,
            transpose,
        } => serde_json::json!({
            "type": "trigger", "voice": voice, "macro": macro_number, "note": note,
            "volume": volume, "transpose": transpose
        }),
        TraceEvent::Voice { voice, state } => serde_json::json!({
            "type": "voice", "voice": voice, "state": format!("{state:?}")
        }),
    };
    writeln!(out, "{value}")
}
