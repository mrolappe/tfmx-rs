//! `Module` — parses and borrows the `mdat`/`smpl` header data.
//!
//! See `docs/format.md` §2 for the byte layout this implements.

const MAGIC: &[u8; 10] = b"TFMX-SONG ";
const HEADER_LEN: usize = 0x1DC;
const TEXT_OFFSET: usize = 0x010;
const TEXT_LEN: usize = 240;
const SONG_START_OFFSET: usize = 0x100;
const SONG_END_OFFSET: usize = 0x140;
const TEMPO_OFFSET: usize = 0x180;
const TABLE_ENTRIES: usize = 32;

const LAYOUT_TABLE_OFFSET: usize = 0x1D0;
const FIXED_PATTERN_PTR_OFFSET: u32 = 0x400;
const FIXED_MACRO_PTR_OFFSET: u32 = 0x600;
const FIXED_TRACKSTEP_OFFSET: u32 = 0x800;

// docs/format.md §6: "maximum of 128 patterns per song file" [S1], stated
// for both layouts. §9: the same 128-entry size for macros is inferred from
// the corpus only (the fixed layout's macro-pointer table spans exactly
// $600-$800), not stated by [S1]; applied here to both layouts for lack of
// a documented alternative.
const MAX_PATTERNS: u8 = 128;
const MAX_MACROS: u8 = 128;
const POINTER_ENTRY_LEN: usize = 4;
const TRACKSTEP_LINE_LEN: usize = 16;

/// Which of the two on-disk header layouts a module uses. See
/// `docs/format.md` §3: detection is a plain zero check on the three longs
/// at `$1D0`, not a heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// The three longs at `$1D0` are all zero; tables sit at fixed offsets
    /// (`docs/format.md` §3.2).
    Fixed,
    /// The three longs at `$1D0` are explicit in-file offsets
    /// (`docs/format.md` §3.1).
    Packed,
}

/// A parsed `mdat`/`smpl` pair. Borrows both buffers; never copies.
#[derive(Debug)]
pub struct Module<'a> {
    mdat: &'a [u8],
    smpl: &'a [u8],
    text: &'a [u8],
    song_start: [u16; TABLE_ENTRIES],
    song_end: [u16; TABLE_ENTRIES],
    tempo: [u16; TABLE_ENTRIES],
    layout: Layout,
    trackstep_offset: u32,
    pattern_ptr_offset: u32,
    macro_ptr_offset: u32,
}

/// Why [`Module::parse`] failed.
#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    /// `mdat` is shorter than the fixed header ($0–$1C0).
    TooShort,
    /// `mdat` does not start with `"TFMX-SONG "`.
    BadMagic,
}

/// Why a [`Module`] accessor (`pattern`, `macro_`, `sample`) failed: an
/// index or a file-derived offset fell outside the buffer it indexes into.
/// This is the trust boundary for untrusted `mdat`/`smpl` input --
/// `docs/architecture.md` §5.
#[derive(Debug, PartialEq, Eq)]
pub enum AccessError {
    OutOfRange,
}

impl<'a> Module<'a> {
    /// Parses the fixed `mdat` header: magic, free-text area, the 96-word
    /// song-start/song-end/tempo table, and the `$1D0` layout table.
    pub fn parse(mdat: &'a [u8], smpl: &'a [u8]) -> Result<Module<'a>, ParseError> {
        if mdat.len() < HEADER_LEN {
            return Err(ParseError::TooShort);
        }
        if &mdat[0..10] != MAGIC {
            return Err(ParseError::BadMagic);
        }

        let raw_trackstep = read_long(mdat, LAYOUT_TABLE_OFFSET);
        let raw_pattern_ptr = read_long(mdat, LAYOUT_TABLE_OFFSET + 4);
        let raw_macro_ptr = read_long(mdat, LAYOUT_TABLE_OFFSET + 8);

        let (layout, trackstep_offset, pattern_ptr_offset, macro_ptr_offset) =
            if raw_trackstep == 0 && raw_pattern_ptr == 0 && raw_macro_ptr == 0 {
                (
                    Layout::Fixed,
                    FIXED_TRACKSTEP_OFFSET,
                    FIXED_PATTERN_PTR_OFFSET,
                    FIXED_MACRO_PTR_OFFSET,
                )
            } else {
                (
                    Layout::Packed,
                    raw_trackstep,
                    raw_pattern_ptr,
                    raw_macro_ptr,
                )
            };

        Ok(Module {
            mdat,
            smpl,
            text: &mdat[TEXT_OFFSET..TEXT_OFFSET + TEXT_LEN],
            song_start: read_word_table(mdat, SONG_START_OFFSET),
            song_end: read_word_table(mdat, SONG_END_OFFSET),
            tempo: read_word_table(mdat, TEMPO_OFFSET),
            layout,
            trackstep_offset,
            pattern_ptr_offset,
            macro_ptr_offset,
        })
    }

    /// The 40x6 free-text area: raw space-padded ASCII bytes, as stored.
    pub fn text(&self) -> &'a [u8] {
        self.text
    }

    /// Trackstep line index where song `n` (0–31) begins.
    pub fn song_start(&self, n: u8) -> u16 {
        self.song_start[n as usize]
    }

    /// Trackstep line index where song `n` (0–31) ends.
    pub fn song_end(&self, n: u8) -> u16 {
        self.song_end[n as usize]
    }

    /// Tempo value for slot `n` (0–31). See `docs/playback-model.md` §3.2
    /// for how to interpret it (50 Hz-divider path vs. BPM path).
    pub fn tempo(&self, n: u8) -> u16 {
        self.tempo[n as usize]
    }

    /// Which on-disk header layout this module uses.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// Absolute `mdat` byte offset of the trackstep table.
    pub fn trackstep_offset(&self) -> u32 {
        self.trackstep_offset
    }

    /// Absolute `mdat` byte offset of the pattern-pointer table.
    pub fn pattern_ptr_offset(&self) -> u32 {
        self.pattern_ptr_offset
    }

    /// Absolute `mdat` byte offset of the macro-pointer table.
    pub fn macro_ptr_offset(&self) -> u32 {
        self.macro_ptr_offset
    }

    /// Pattern `n`'s data, from its start to the end of `mdat` -- the
    /// pattern decoder (step 4.3) walks it longword by longword until it
    /// hits an `$F0` End command; there is no length field to bounds it
    /// more tightly. `docs/format.md` §6.
    pub fn pattern(&self, n: u8) -> Result<&'a [u8], AccessError> {
        let offset = pointer_table_offset(self.mdat, self.pattern_ptr_offset, n, MAX_PATTERNS)?;
        self.mdat
            .get(offset as usize..)
            .ok_or(AccessError::OutOfRange)
    }

    /// Macro `n`'s data, from its start to the end of `mdat`. Same shape as
    /// [`Module::pattern`]; the macro interpreter (step 4.4) terminates on
    /// `$07 STOP`. `docs/format.md` §7.
    pub fn macro_(&self, n: u8) -> Result<&'a [u8], AccessError> {
        let offset = pointer_table_offset(self.mdat, self.macro_ptr_offset, n, MAX_MACROS)?;
        self.mdat
            .get(offset as usize..)
            .ok_or(AccessError::OutOfRange)
    }

    /// Absolute `mdat` byte offset of pattern `n`'s data -- the same offset
    /// [`Module::pattern`] slices from, exposed for callers (the static
    /// walker, `docs/m5-plan.md` Phase 5.2) that need to know *where* a
    /// pattern lives, not just its bytes, to report byte-provenance spans.
    pub fn pattern_offset(&self, n: u8) -> Result<u32, AccessError> {
        pointer_table_offset(self.mdat, self.pattern_ptr_offset, n, MAX_PATTERNS)
    }

    /// Absolute `mdat` byte offset of macro `n`'s data. See
    /// [`Module::pattern_offset`].
    pub fn macro_offset(&self, n: u8) -> Result<u32, AccessError> {
        pointer_table_offset(self.mdat, self.macro_ptr_offset, n, MAX_MACROS)
    }

    /// Trackstep line `line`'s 16 raw bytes (8 words, one per track).
    /// `docs/format.md` §5; decoding the words is the trackstep runner's job
    /// (step 4.2), not this accessor's.
    pub fn trackstep_line(&self, line: u16) -> Result<&'a [u8; 16], AccessError> {
        let start = (self.trackstep_offset as usize)
            .checked_add(line as usize * TRACKSTEP_LINE_LEN)
            .ok_or(AccessError::OutOfRange)?;
        let end = start
            .checked_add(TRACKSTEP_LINE_LEN)
            .ok_or(AccessError::OutOfRange)?;
        let bytes = self.mdat.get(start..end).ok_or(AccessError::OutOfRange)?;
        Ok(bytes.try_into().expect("slice of TRACKSTEP_LINE_LEN bytes"))
    }

    /// Signed 8-bit PCM sample bytes `[offset, offset+len)` from `smpl`.
    /// `docs/format.md` §8.
    pub fn sample(&self, offset: u32, len: u32) -> Result<&'a [i8], AccessError> {
        let start = offset as usize;
        let end = start
            .checked_add(len as usize)
            .ok_or(AccessError::OutOfRange)?;
        let bytes = self.smpl.get(start..end).ok_or(AccessError::OutOfRange)?;
        // Safety: i8 and u8 have identical size, alignment and bit validity;
        // this only reinterprets the sign of each byte.
        Ok(unsafe { core::slice::from_raw_parts(bytes.as_ptr().cast::<i8>(), bytes.len()) })
    }

    /// The entire `smpl` buffer as signed 8-bit PCM. `Player` (step 4.4)
    /// holds this once and passes it to `Paula::render` on every chunk,
    /// since `Voice` register values are offsets anywhere within it --
    /// `docs/architecture.md` §2. `docs/format.md` §8.
    pub fn smpl(&self) -> &'a [i8] {
        // Safety: same reinterpret-cast as `sample`, over the whole buffer.
        unsafe { core::slice::from_raw_parts(self.smpl.as_ptr().cast::<i8>(), self.smpl.len()) }
    }
}

fn pointer_table_offset(
    mdat: &[u8],
    table_offset: u32,
    n: u8,
    max: u8,
) -> Result<u32, AccessError> {
    if n >= max {
        return Err(AccessError::OutOfRange);
    }
    let entry_offset = (table_offset as usize)
        .checked_add(n as usize * POINTER_ENTRY_LEN)
        .ok_or(AccessError::OutOfRange)?;
    let entry_end = entry_offset
        .checked_add(POINTER_ENTRY_LEN)
        .ok_or(AccessError::OutOfRange)?;
    if entry_end > mdat.len() {
        return Err(AccessError::OutOfRange);
    }
    Ok(read_long(mdat, entry_offset))
}

fn read_word_table(mdat: &[u8], offset: usize) -> [u16; TABLE_ENTRIES] {
    let mut table = [0u16; TABLE_ENTRIES];
    for (i, slot) in table.iter_mut().enumerate() {
        let o = offset + i * 2;
        *slot = u16::from_be_bytes([mdat[o], mdat[o + 1]]);
    }
    table
}

fn read_long(mdat: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes([
        mdat[offset],
        mdat[offset + 1],
        mdat[offset + 2],
        mdat[offset + 3],
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        fs::read(path).ok()
    }

    #[test]
    fn parses_known_file_header() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");

        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        assert!(module.text().starts_with(b"(Empty)"));
        assert_eq!(module.text().len(), 240);
        assert_eq!(module.song_start(0), 75);
        assert_eq!(module.song_end(0), 129);
        assert_eq!(module.tempo(0), 3);
        assert_eq!(module.tempo(1), 120);
        assert_eq!(module.tempo(2), 160);
    }

    #[test]
    fn rejects_bad_magic() {
        let mut mdat = vec![0u8; HEADER_LEN];
        mdat[0..10].copy_from_slice(b"NOT-A-TFMX");
        assert_eq!(Module::parse(&mdat, &[]).unwrap_err(), ParseError::BadMagic);
    }

    #[test]
    fn rejects_truncated_header() {
        let mut mdat = vec![0u8; HEADER_LEN - 1];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        assert_eq!(Module::parse(&mdat, &[]).unwrap_err(), ParseError::TooShort);
    }

    #[test]
    fn detects_fixed_layout() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        assert_eq!(module.layout(), Layout::Fixed);
        assert_eq!(module.trackstep_offset(), 0x800);
        assert_eq!(module.pattern_ptr_offset(), 0x400);
        assert_eq!(module.macro_ptr_offset(), 0x600);
    }

    #[test]
    fn detects_packed_layout() {
        let Some(mdat) = read_corpus("mdat.turrican 2 level 1-desert") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl =
            read_corpus("smpl.turrican 2 level 1-desert").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        assert_eq!(module.layout(), Layout::Packed);
        assert_eq!(module.trackstep_offset(), 0x3E8);
        assert_eq!(module.pattern_ptr_offset(), 0x3078);
        assert_eq!(module.macro_ptr_offset(), 0x31DC);
    }

    #[test]
    fn all_corpus_files_parse_with_documented_layout() {
        // Layout column of testdata/README.md.
        let files: [(&str, Layout); 10] = [
            ("turrican intro", Layout::Fixed),
            ("turrican outside", Layout::Fixed),
            ("r-type", Layout::Fixed),
            ("x-out (title)", Layout::Fixed),
            ("turrican 2 title (st)", Layout::Fixed),
            ("turrican 2 level 1-desert", Layout::Packed),
            ("turrican 2 level 3-flight", Layout::Packed),
            ("turrican 3 level 1", Layout::Packed),
            ("apidya (title)", Layout::Packed),
            ("apidya (level 1)", Layout::Packed),
        ];

        for (name, expected) in files {
            let Some(mdat) = read_corpus(&format!("mdat.{name}")) else {
                eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
                return;
            };
            let smpl = read_corpus(&format!("smpl.{name}")).expect("smpl present alongside mdat");
            let module = Module::parse(&mdat, &smpl).unwrap_or_else(|e| panic!("{name}: {e:?}"));
            assert_eq!(module.layout(), expected, "{name}");
        }
    }

    #[test]
    fn pattern_and_macro_access_known_file() {
        let Some(mdat) = read_corpus("mdat.turrican 2 level 1-desert") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl =
            read_corpus("smpl.turrican 2 level 1-desert").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        // docs/format.md §6: pattern-pointer table at $3078, entry 0 = $00000A48,
        // whose first longword decodes as 98 2F 50 07.
        let pattern0 = module.pattern(0).expect("pattern 0 in range");
        assert_eq!(&pattern0[0..4], &[0x98, 0x2F, 0x50, 0x07]);

        // docs/format.md §7: macro-pointer table at $31DC, entry 0 = $000022EC,
        // whose first longword decodes as 00 00 00 00.
        let macro0 = module.macro_(0).expect("macro 0 in range");
        assert_eq!(&macro0[0..4], &[0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn pattern_and_macro_offset_known_file() {
        let Some(mdat) = read_corpus("mdat.turrican 2 level 1-desert") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl =
            read_corpus("smpl.turrican 2 level 1-desert").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        // Same entries as `pattern_and_macro_access_known_file`, but checking
        // the raw offset the accessor derives the slice from, not the bytes.
        assert_eq!(
            module.pattern_offset(0).expect("pattern 0 in range"),
            0x00000A48
        );
        assert_eq!(
            module.macro_offset(0).expect("macro 0 in range"),
            0x000022EC
        );
    }

    #[test]
    fn pattern_and_macro_offset_out_of_range_is_err_not_panic() {
        let mut mdat = vec![0u8; HEADER_LEN];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let module = Module::parse(&mdat, &[]).expect("minimal header parses");

        assert_eq!(
            module.pattern_offset(128).unwrap_err(),
            AccessError::OutOfRange
        );
        assert_eq!(
            module.macro_offset(128).unwrap_err(),
            AccessError::OutOfRange
        );
        assert_eq!(
            module.pattern_offset(0).unwrap_err(),
            AccessError::OutOfRange
        );
    }

    #[test]
    fn pattern_index_out_of_range_is_err_not_panic() {
        let mut mdat = vec![0u8; HEADER_LEN];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let module = Module::parse(&mdat, &[]).expect("minimal header parses");

        assert_eq!(module.pattern(128).unwrap_err(), AccessError::OutOfRange);
        assert_eq!(module.macro_(128).unwrap_err(), AccessError::OutOfRange);
    }

    #[test]
    fn corrupted_pointer_table_is_err_not_panic() {
        // Fixed layout: pattern pointers are claimed to live at $400, but
        // this buffer ends at HEADER_LEN ($1DC) -- reading entry 0 there,
        // or following whatever garbage offset it might contain, must not
        // panic even though nothing valid is actually present.
        let mut mdat = vec![0u8; HEADER_LEN];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let module = Module::parse(&mdat, &[]).expect("minimal header parses");

        assert_eq!(module.pattern(0).unwrap_err(), AccessError::OutOfRange);
        assert_eq!(module.macro_(0).unwrap_err(), AccessError::OutOfRange);
    }

    #[test]
    fn trackstep_line_known_file() {
        let Some(mdat) = read_corpus("mdat.turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl = read_corpus("smpl.turrican intro").expect("smpl present alongside mdat");
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        // Song 0 starts at line 75; `$800 + 75*16 = $838` reads a $EFFE
        // MasterVolSlide(B) command, verified by direct byte inspection.
        assert_eq!(module.song_start(0), 75);
        let line = module.trackstep_line(75).expect("line in range");
        assert_eq!(
            line,
            &[
                0xEF, 0xFE, 0x00, 0x04, 0x00, 0x00, 0x00, 0x40, 0xFF, 0x00, 0xFF, 0x00, 0xFF, 0x00,
                0xFF, 0x00
            ]
        );
    }

    #[test]
    fn trackstep_line_out_of_range_is_err_not_panic() {
        let mut mdat = vec![0u8; HEADER_LEN];
        mdat[0..10].copy_from_slice(b"TFMX-SONG ");
        let module = Module::parse(&mdat, &[]).expect("minimal header parses");

        assert_eq!(
            module.trackstep_line(u16::MAX).unwrap_err(),
            AccessError::OutOfRange
        );
    }

    #[test]
    fn sample_access_known_file() {
        let Some(mdat) = read_corpus("mdat.turrican 2 level 1-desert") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let smpl =
            read_corpus("smpl.turrican 2 level 1-desert").expect("smpl present alongside mdat");
        let smpl_len = smpl.len() as u32;
        let module = Module::parse(&mdat, &smpl).expect("valid header parses");

        let sample = module.sample(0, 4).expect("in-range sample slice");
        assert_eq!(sample.len(), 4);

        assert_eq!(
            module.sample(smpl_len - 1, 2).unwrap_err(),
            AccessError::OutOfRange
        );
        assert_eq!(
            module.sample(0, u32::MAX).unwrap_err(),
            AccessError::OutOfRange
        );
    }
}
