//! File-wide module info (`docs/gui-plan.md` Phase W3): which song slots are
//! actually used, and which patterns/macros any of them can reach. Neither
//! songs, patterns nor macros carry a per-slot "used" flag in the format, so
//! "used" is defined operationally: a song slot is used when `end > start`
//! (`docs/format.md` §2.2 -- unused slots repeat `start == end` in every
//! corpus file checked, e.g. `mdat.turrican intro`'s slots 3-30), and a
//! pattern/macro is used when [`walk_song`] reaches it from some used song.

use std::collections::BTreeSet;

use tfmx::Module;

use crate::walker::walk_song;

/// One used song slot's start/end trackstep-line range and tempo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SongInfo {
    pub number: u8,
    pub start: u16,
    pub end: u16,
    pub tempo: u16,
}

/// Every used song, and the patterns/macros reachable from any of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ModuleInfo {
    pub songs: Vec<SongInfo>,
    pub patterns: Vec<u8>,
    pub macros: Vec<u8>,
}

/// The 96-word song table holds 32 slots (`docs/format.md` §2.2). A song
/// whose walk fails (a corrupted trackstep offset) is skipped rather than
/// failing the whole listing -- the same per-item resilience
/// `tfmx-cli/src/export/mod.rs`'s `build_instrument` already applies to a
/// zone that doesn't fit `smpl`.
pub fn build_module_info(module: &Module) -> ModuleInfo {
    let mut songs = Vec::new();
    let mut patterns = BTreeSet::new();
    let mut macros = BTreeSet::new();

    for n in 0..32u8 {
        let start = module.song_start(n);
        let end = module.song_end(n);
        if end <= start {
            continue;
        }
        songs.push(SongInfo {
            number: n,
            start,
            end,
            tempo: module.tempo(n),
        });
        if let Ok(walk) = walk_song(module, n) {
            patterns.extend(walk.reachable_patterns);
            macros.extend(walk.reachable_macros);
        }
    }

    ModuleInfo {
        songs,
        patterns: patterns.into_iter().collect(),
        macros: macros.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read(path).ok()
    }

    #[test]
    fn turrican_intro_has_exactly_three_used_songs() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid corpus file");

        let info = build_module_info(&module);

        // `tfmx-cli info`'s own output for this file: songs 0-2 have
        // end > start (75..129, 52..74, 0..49); every other slot repeats
        // start == end (mostly 50..50), confirmed by manual inspection.
        assert_eq!(
            info.songs.iter().map(|s| s.number).collect::<Vec<_>>(),
            vec![0, 1, 2]
        );
        assert_eq!(
            info.songs[0],
            SongInfo {
                number: 0,
                start: 75,
                end: 129,
                tempo: 3
            }
        );
        assert_eq!(
            info.songs[1],
            SongInfo {
                number: 1,
                start: 52,
                end: 74,
                tempo: 120
            }
        );
        assert_eq!(
            info.songs[2],
            SongInfo {
                number: 2,
                start: 0,
                end: 49,
                tempo: 160
            }
        );

        assert!(
            !info.patterns.is_empty(),
            "song 0-2 reach at least one pattern"
        );
        assert!(!info.macros.is_empty(), "song 0-2 reach at least one macro");
        assert!(
            info.patterns.windows(2).all(|w| w[0] < w[1]),
            "patterns are sorted and deduplicated"
        );
        assert!(
            info.macros.windows(2).all(|w| w[0] < w[1]),
            "macros are sorted and deduplicated"
        );
    }

    #[test]
    fn an_empty_module_has_no_used_songs() {
        // A minimal header: magic + zeroed rest means every song slot's
        // start and end both read 0, so none qualify as used.
        let mut mdat = vec![0u8; 0x400];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let module = Module::parse(&mdat, &[]).expect("minimal header parses");

        let info = build_module_info(&module);

        assert!(info.songs.is_empty());
        assert!(info.patterns.is_empty());
        assert!(info.macros.is_empty());
    }
}
