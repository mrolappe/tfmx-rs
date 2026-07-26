//! Thin `#[wasm_bindgen]` shell over `Core` (step 8.1): marshals
//! `&[u8]`/typed-array arguments and maps `Error` to `JsError`. Only
//! compiled for `wasm32` (see `lib.rs`) -- `wasm_bindgen`-attributed types
//! only compile and run with the JS glue `wasm-bindgen` generates, so they
//! can't be exercised by plain host `cargo test`.

use wasm_bindgen::prelude::*;

use crate::Core;

#[wasm_bindgen]
pub struct TfmxWeb {
    core: Core,
}

#[wasm_bindgen]
impl TfmxWeb {
    /// Parses `mdat`/`smpl` and starts at song 0, full stereo separation --
    /// the demo page (step 10.1) has no separation control, and `set_song`
    /// covers song selection after construction.
    #[wasm_bindgen(constructor)]
    pub fn new(mdat: &[u8], smpl: &[u8], sample_rate: u32) -> Result<TfmxWeb, JsError> {
        let core = Core::new(mdat.to_vec(), smpl.to_vec(), 0, sample_rate, 100)
            .map_err(|e| JsError::new(&e.to_string()))?;
        Ok(TfmxWeb { core })
    }

    /// Fills `out` (interleaved stereo `i16`) with `out.len() / 2` frames.
    pub fn render(&mut self, out: &mut [i16]) -> Result<(), JsError> {
        self.core.render(out).map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn set_song(&mut self, song: u32) -> Result<(), JsError> {
        self.core
            .set_song(song)
            .map_err(|e| JsError::new(&e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasm_bindgen_test::*;

    // Node is `wasm-bindgen-test`'s default target -- no
    // `wasm_bindgen_test_configure!` call needed, that macro only exists to
    // opt into `run_in_browser`.

    // `include_bytes!` runs at compile time on the host filesystem, not at
    // wasm runtime -- wasm32-unknown-unknown has no `std::fs`, so this is
    // the only way to get corpus fixtures into a `wasm-bindgen-test` binary.
    // Requires `sh testdata/fetch.sh` to have been run first, unlike the
    // host tests in `lib.rs` which skip gracefully when it hasn't.
    const MDAT: &[u8] = include_bytes!("../../testdata/mdat.turrican intro");
    const SMPL: &[u8] = include_bytes!("../../testdata/smpl.turrican intro");

    #[wasm_bindgen_test]
    fn constructs_renders_and_switches_song() {
        let mut web = TfmxWeb::new(MDAT, SMPL, 44_100).expect("valid corpus module");

        // Song 0 in this corpus file has tempo=3 (very slow, see
        // lib.rs's own render test) -- a full second, not a small block,
        // is needed to guarantee the first audible note has landed.
        let mut out = [0i16; 44_100 * 2];
        web.render(&mut out).expect("in-range corpus render");
        assert!(out.iter().any(|&s| s != 0), "render must not be silent");

        web.set_song(1).expect("song 1 in range");
    }
}
