//! Route handlers over a loaded [`Session`] (`docs/gui-plan.md` Phase W1).
//! Every handler returns a plain `tiny_http::Response<Cursor<Vec<u8>>>` --
//! the same type `Response::from_data`/`from_string` already produce, so
//! `main.rs`'s dispatch match needs no wrapper type of its own.

use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use tfmx_analysis::DisasmLineView;
use tiny_http::{Header, Response};

use crate::session::Session;

type ApiResponse = Response<Cursor<Vec<u8>>>;

fn json_response(status: u16, value: serde_json::Value) -> ApiResponse {
    let body = serde_json::to_vec(&value).expect("serde_json::Value always serializes");
    let header = Header::from_bytes(&b"Content-Type"[..], b"application/json")
        .expect("static header bytes are valid");
    Response::from_data(body)
        .with_status_code(status)
        .with_header(header)
}

fn error_response(status: u16, message: impl std::fmt::Display) -> ApiResponse {
    json_response(status, serde_json::json!({ "error": message.to_string() }))
}

fn wav_response(pcm: &[i16], rate: u32) -> ApiResponse {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut bytes = Vec::new();
    {
        let mut writer =
            hound::WavWriter::new(Cursor::new(&mut bytes), spec).expect("spec is valid");
        for &sample in pcm {
            writer
                .write_sample(sample)
                .expect("write to an in-memory buffer cannot fail");
        }
        writer.finalize().expect("finalize an in-memory buffer");
    }
    let header = Header::from_bytes(&b"Content-Type"[..], b"audio/wav")
        .expect("static header bytes are valid");
    Response::from_data(bytes).with_header(header)
}

fn query_u8(query: &HashMap<String, String>, key: &str) -> Option<u8> {
    query.get(key).and_then(|v| v.parse().ok())
}

fn query_num<T: std::str::FromStr>(query: &HashMap<String, String>, key: &str, default: T) -> T {
    query
        .get(key)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// `GET /files?dir=` -- every `mdat.*`/`smpl.*` pair found directly under
/// `dir` (default: cwd), the naming convention `testdata/fetch.sh` and every
/// corpus module already follow.
pub fn list_files(query: &HashMap<String, String>) -> ApiResponse {
    let dir = query.get("dir").map(String::as_str).unwrap_or(".");
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return error_response(400, format!("reading {dir}: {e}")),
    };

    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|name| name.strip_prefix("mdat.").map(str::to_string))
        .collect();
    names.sort();

    let pairs: Vec<serde_json::Value> = names
        .into_iter()
        .filter(|name| Path::new(dir).join(format!("smpl.{name}")).exists())
        .map(|name| {
            serde_json::json!({
                "name": name,
                "mdat_path": Path::new(dir).join(format!("mdat.{name}")),
                "smpl_path": Path::new(dir).join(format!("smpl.{name}")),
            })
        })
        .collect();
    json_response(200, serde_json::Value::Array(pairs))
}

/// `POST /load` -- body `{"mdat_path": ..., "smpl_path": ...}`. Replaces
/// whatever module was loaded before.
pub fn load(session: &mut Option<Session>, body: &str) -> ApiResponse {
    let value: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return error_response(400, format!("invalid JSON body: {e}")),
    };
    let (Some(mdat), Some(smpl)) = (
        value.get("mdat_path").and_then(|v| v.as_str()),
        value.get("smpl_path").and_then(|v| v.as_str()),
    ) else {
        return error_response(
            400,
            "expected a JSON body {\"mdat_path\": ..., \"smpl_path\": ...}",
        );
    };

    match Session::load(PathBuf::from(mdat), PathBuf::from(smpl)) {
        Ok(loaded) => {
            let response = json_response(
                200,
                serde_json::json!({
                    "ok": true,
                    "mdat_path": loaded.mdat_path(),
                    "smpl_path": loaded.smpl_path(),
                }),
            );
            *session = Some(loaded);
            response
        }
        Err(e) => error_response(400, e.to_string()),
    }
}

/// `GET /song-view?song=` -- [`tfmx_analysis::build_song_view`] as JSON.
pub fn song_view(session: Option<&Session>, query: &HashMap<String, String>) -> ApiResponse {
    let Some(session) = session else {
        return error_response(400, "no module loaded; POST /load first");
    };
    let song = query_num(query, "song", 0u8);
    let module = session.module();
    match tfmx_analysis::build_song_view(&module, song) {
        Ok(view) => json_response(
            200,
            serde_json::to_value(view).expect("SongView always serializes"),
        ),
        Err(e) => error_response(400, format!("{e:?}")),
    }
}

/// `GET /disasm?macro=` / `?pattern=` -- a [`DisasmLineView`] listing as
/// JSON.
pub fn disasm(session: Option<&Session>, query: &HashMap<String, String>) -> ApiResponse {
    let Some(session) = session else {
        return error_response(400, "no module loaded; POST /load first");
    };
    let module = session.module();
    let lines = match (query_u8(query, "macro"), query_u8(query, "pattern")) {
        (Some(macro_number), None) => tfmx_analysis::disassemble_macro(&module, macro_number),
        (None, Some(pattern)) => tfmx_analysis::disassemble_pattern(&module, pattern),
        _ => return error_response(400, "exactly one of ?macro=/?pattern= is required"),
    };
    match lines {
        Ok(lines) => {
            let views: Vec<DisasmLineView> = lines.into_iter().map(DisasmLineView::from).collect();
            json_response(
                200,
                serde_json::to_value(views).expect("DisasmLineView always serializes"),
            )
        }
        Err(e) => error_response(400, format!("{e:?}")),
    }
}

/// `GET /render-macro?macro=&note=&volume=&voice=&tempo=&seconds=&rate=&separation=`
/// -- a WAV rendering of one triggered macro, same defaults as `tfmx-cli
/// render-macro`.
pub fn render_macro(session: Option<&Session>, query: &HashMap<String, String>) -> ApiResponse {
    let Some(session) = session else {
        return error_response(400, "no module loaded; POST /load first");
    };
    let Some(macro_number) = query_u8(query, "macro") else {
        return error_response(400, "?macro= is required");
    };
    let note = query_num(query, "note", 30u8); // C-3
    let volume = query_num(query, "volume", 64u8);
    let voice = query_num(query, "voice", 0u8);
    let tempo = query_num(query, "tempo", 0u16);
    let seconds = query_num(query, "seconds", 5u32);
    let rate = query_num(query, "rate", 44_100u32);
    let separation = query_num(query, "separation", 100u8);

    let module = session.module();
    let total_frames = rate as usize * seconds as usize;
    match tfmx_analysis::render_macro_pcm(
        &module,
        macro_number,
        note,
        volume,
        voice,
        tempo,
        rate,
        separation,
        total_frames,
    ) {
        Ok(pcm) => wav_response(&pcm, rate),
        Err(e) => error_response(400, format!("{e:?}")),
    }
}

/// `GET /render-pattern?pattern=&transpose=&tempo=&seconds=&rate=&separation=`
/// -- a WAV rendering of one standalone pattern, same defaults as `tfmx-cli
/// render-pattern`.
pub fn render_pattern(session: Option<&Session>, query: &HashMap<String, String>) -> ApiResponse {
    let Some(session) = session else {
        return error_response(400, "no module loaded; POST /load first");
    };
    let Some(pattern) = query_u8(query, "pattern") else {
        return error_response(400, "?pattern= is required");
    };
    let transpose = query_num(query, "transpose", 0i8);
    let tempo = query_num(query, "tempo", 0u16);
    let seconds = query_num(query, "seconds", 10u32);
    let rate = query_num(query, "rate", 44_100u32);
    let separation = query_num(query, "separation", 100u8);

    let module = session.module();
    let total_frames = rate as usize * seconds as usize;
    match tfmx_analysis::render_pattern_pcm(
        &module,
        pattern,
        transpose,
        tempo,
        rate,
        separation,
        total_frames,
    ) {
        Ok(pcm) => wav_response(&pcm, rate),
        Err(e) => error_response(400, format!("{e:?}")),
    }
}

pub fn not_found() -> ApiResponse {
    error_response(404, "not found")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn testdata_dir() -> PathBuf {
        PathBuf::from(format!("{}/../testdata", env!("CARGO_MANIFEST_DIR")))
    }

    fn corpus_paths() -> Option<(PathBuf, PathBuf)> {
        let mdat = testdata_dir().join("mdat.turrican intro");
        let smpl = testdata_dir().join("smpl.turrican intro");
        mdat.exists().then_some((mdat, smpl))
    }

    fn loaded_session() -> Option<Session> {
        let (mdat, smpl) = corpus_paths()?;
        Some(Session::load(mdat, smpl).expect("valid corpus pair loads"))
    }

    fn status_and_json(response: ApiResponse) -> (u16, serde_json::Value) {
        let status = response.status_code().0;
        let bytes = response.into_reader().into_inner();
        (
            status,
            serde_json::from_slice(&bytes).expect("valid JSON body"),
        )
    }

    #[test]
    fn list_files_finds_the_known_corpus_pair() {
        let dir = testdata_dir();
        if !dir.join("mdat.turrican intro").exists() {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        }
        let query = HashMap::from([("dir".to_string(), dir.to_string_lossy().into_owned())]);
        let (status, body) = status_and_json(list_files(&query));
        assert_eq!(status, 200);
        let names: Vec<&str> = body
            .as_array()
            .expect("array of pairs")
            .iter()
            .map(|entry| entry["name"].as_str().expect("name is a string"))
            .collect();
        assert!(names.contains(&"turrican intro"));
    }

    #[test]
    fn list_files_reports_an_error_for_a_missing_directory() {
        let query = HashMap::from([("dir".to_string(), "/does/not/exist".to_string())]);
        let (status, body) = status_and_json(list_files(&query));
        assert_eq!(status, 400);
        assert!(body["error"].is_string());
    }

    #[test]
    fn load_rejects_a_malformed_body() {
        let mut session = None;
        let (status, body) = status_and_json(load(&mut session, "not json"));
        assert_eq!(status, 400);
        assert!(body["error"].is_string());
        assert!(session.is_none());
    }

    #[test]
    fn load_accepts_a_valid_pair_and_the_session_becomes_usable() {
        let Some((mdat, smpl)) = corpus_paths() else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let mut session = None;
        let body = serde_json::json!({
            "mdat_path": mdat.to_string_lossy(),
            "smpl_path": smpl.to_string_lossy(),
        })
        .to_string();
        let (status, response_body) = status_and_json(load(&mut session, &body));
        assert_eq!(status, 200);
        assert_eq!(response_body["ok"], true);
        assert!(session.is_some());
    }

    #[test]
    fn song_view_without_a_loaded_session_is_an_error() {
        let query = HashMap::new();
        let (status, body) = status_and_json(song_view(None, &query));
        assert_eq!(status, 400);
        assert!(body["error"].is_string());
    }

    #[test]
    fn song_view_returns_song_zeros_waveform_and_trackstep_data() {
        let Some(session) = loaded_session() else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let query = HashMap::from([("song".to_string(), "0".to_string())]);
        let (status, body) = status_and_json(song_view(Some(&session), &query));
        assert_eq!(status, 200);
        assert_eq!(body["song"], 0);
        assert!(body["waveform"]["regions"].is_array());
        assert!(body["trackstep"]["steps"].is_array());
    }

    #[test]
    fn disasm_requires_exactly_one_of_macro_or_pattern() {
        let query = HashMap::new();
        let (status, body) = status_and_json(disasm(None, &query));
        assert_eq!(status, 400);
        assert!(body["error"].is_string());
    }

    #[test]
    fn disasm_matches_the_known_decode_of_pattern_84_step_0() {
        let Some(session) = loaded_session() else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let query = HashMap::from([("pattern".to_string(), "84".to_string())]);
        let (status, body) = status_and_json(disasm(Some(&session), &query));
        assert_eq!(status, 200);
        let note = &body[0]["Pattern"]["entry"]["Note"];
        assert_eq!(note["note"], 33);
        assert_eq!(note["macro_number"], 48);
        assert_eq!(note["volume"], 12);
        assert_eq!(note["voice"], 2);
        assert_eq!(note["timing"], serde_json::json!({ "Wait": 31 }));
    }

    #[test]
    fn render_macro_produces_a_wav_of_the_requested_length() {
        let Some(session) = loaded_session() else {
            eprintln!("skipping: run `sh testdata/fetch.sh` to fetch the test corpus");
            return;
        };
        let query = HashMap::from([
            ("macro".to_string(), "28".to_string()),
            ("seconds".to_string(), "1".to_string()),
            ("rate".to_string(), "8000".to_string()),
        ]);
        let response = render_macro(Some(&session), &query);
        assert_eq!(response.status_code().0, 200);
        let bytes = response.into_reader().into_inner();
        let reader = hound::WavReader::new(Cursor::new(bytes)).expect("valid WAV");
        assert_eq!(reader.spec().sample_rate, 8000);
        assert_eq!(reader.spec().channels, 2);
        assert_eq!(reader.len(), 8000 * 2);
    }

    #[test]
    fn render_pattern_requires_a_pattern_parameter() {
        let query = HashMap::new();
        let (status, body) = status_and_json(render_pattern(None, &query));
        assert_eq!(status, 400);
        assert!(body["error"].is_string());
    }
}
