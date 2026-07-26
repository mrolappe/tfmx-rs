//! Plain-Rust playback core for `tfmx-web` -- no `wasm_bindgen` attributes,
//! so `cargo test` exercises it directly on the host target
//! (`wasm_bindgen`-attributed types only compile and run under `wasm32`
//! with the JS glue present). Step 8.2 adds a thin `#[wasm_bindgen]` shell
//! around `Core` that only marshals `&[u8]`/`Uint8Array` and maps errors.

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::TfmxWeb;

/// A parsed module and its current `Player`. `mdat`/`smpl` and the parsed
/// `Module` are leaked to `'static` on construction, mirroring `tfmx-play`'s
/// realtime state (step 7.2): a `Core`'s lifetime is already the whole
/// page/tab, so there's no earlier point at which freeing them would
/// matter, and it avoids a self-referential struct for what
/// `Module`/`Player`'s borrow-based design already assumes is a `'static`
/// owner.
pub struct Core {
    module: &'static tfmx::Module<'static>,
    sample_rate: u32,
    separation: u8,
    player: tfmx::Player<'static>,
}

/// Errors from parsing a module or building/rebuilding its `Player`.
#[derive(Debug)]
pub enum Error {
    Parse(tfmx::ParseError),
    Access(tfmx::AccessError),
}

impl From<tfmx::ParseError> for Error {
    fn from(e: tfmx::ParseError) -> Self {
        Error::Parse(e)
    }
}

impl From<tfmx::AccessError> for Error {
    fn from(e: tfmx::AccessError) -> Self {
        Error::Access(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Parse(e) => write!(f, "invalid module: {e:?}"),
            Error::Access(e) => write!(f, "out-of-range access: {e:?}"),
        }
    }
}

impl std::error::Error for Error {}

impl Core {
    /// Parses `mdat`/`smpl` and builds a `Player` for `song` (0-31,
    /// truncated from `u32` -- the header's song-slot table is always 32
    /// entries wide, so it always fits in a `u8`).
    pub fn new(
        mdat: Vec<u8>,
        smpl: Vec<u8>,
        song: u32,
        sample_rate: u32,
        separation: u8,
    ) -> Result<Self, Error> {
        let mdat: &'static [u8] = mdat.leak();
        let smpl: &'static [u8] = smpl.leak();
        let module: &'static tfmx::Module<'static> =
            Box::leak(Box::new(tfmx::Module::parse(mdat, smpl)?));
        let player = tfmx::Player::new(module, song as u8, sample_rate, separation)?;
        Ok(Self {
            module,
            sample_rate,
            separation,
            player,
        })
    }

    /// Fills `out` (interleaved stereo `i16`) with `out.len() / 2` frames.
    pub fn render(&mut self, out: &mut [i16]) -> Result<(), Error> {
        self.player.render(out)?;
        Ok(())
    }

    /// Rebuilds `player` for `song`, keeping the already-parsed `module`
    /// (mirrors `tfmx-play`'s song-switch handling, step 7.2).
    pub fn set_song(&mut self, song: u32) -> Result<(), Error> {
        self.player = tfmx::Player::new(self.module, song as u8, self.sample_rate, self.separation)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_corpus(name: &str) -> Option<Vec<u8>> {
        let path = format!("{}/../testdata/{}", env!("CARGO_MANIFEST_DIR"), name);
        std::fs::read(path).ok()
    }

    fn corpus_pair(stem: &str) -> Option<(Vec<u8>, Vec<u8>)> {
        let mdat = read_corpus(&format!("mdat.{stem}"))?;
        let smpl = read_corpus(&format!("smpl.{stem}")).expect("smpl present alongside mdat");
        Some((mdat, smpl))
    }

    #[test]
    fn constructs_from_a_corpus_module() {
        let Some((mdat, smpl)) = corpus_pair("turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        Core::new(mdat, smpl, 0, 44_100, 100).expect("song 0 of a valid module constructs");
    }

    #[test]
    fn render_produces_non_silent_output() {
        let Some((mdat, smpl)) = corpus_pair("turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let mut core = Core::new(mdat, smpl, 0, 44_100, 100).unwrap();
        let mut out = vec![0i16; 44_100 * 2]; // 1 second, stereo
        core.render(&mut out).expect("in-range corpus render");

        // `i16` output can't carry a `NaN`/`inf` (`Paula::render` clamps
        // through `f64::clamp` before casting), so there is no separate
        // "not NaN" assertion to write here -- see the equivalent note on
        // `tfmx::player`'s own render test.
        assert!(out.iter().any(|&s| s != 0), "render must not be silent");
    }

    #[test]
    fn set_song_switches_to_a_different_song() {
        let Some((mdat, smpl)) = corpus_pair("turrican intro") else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        // Song 0 (start=75 end=129) and song 1 (start=52 end=74) cover
        // disjoint trackstep ranges in this corpus file, so their first
        // second of output is expected to differ.
        let mut core = Core::new(mdat, smpl, 0, 44_100, 100).unwrap();
        let mut before = vec![0i16; 44_100 * 2];
        core.render(&mut before).unwrap();

        core.set_song(1).expect("song 1 in range");
        let mut after = vec![0i16; 44_100 * 2];
        core.render(&mut after).expect("in-range corpus render");

        assert_ne!(before, after, "switching song must change what renders");
    }
}
