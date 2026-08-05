//! Local browser GUI for TFMX tooling (`docs/gui-plan.md`). Phase W0: just
//! the crate skeleton and static-file serving. Routes over a loaded
//! `Session` (`/files`, `/load`, `/song-view`, `/disasm`, `/render-*`) are
//! Phase W1.

// ponytail: `session` isn't wired into a route yet -- that's Phase W1.
#[allow(dead_code)]
mod session;
mod static_files;

use tiny_http::{Response, Server};

fn main() {
    let addr = "127.0.0.1:8080";
    let server = Server::http(addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let static_dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/static"));
    eprintln!("tfmx-web-gui listening on http://{addr}");

    for request in server.incoming_requests() {
        let response = match static_files::resolve(&static_dir, request.url()) {
            Some(path) => {
                let content_type = static_files::content_type(&path);
                let bytes = std::fs::read(&path).unwrap_or_default();
                let header =
                    tiny_http::Header::from_bytes(&b"Content-Type"[..], content_type.as_bytes())
                        .expect("static content-type is valid header bytes");
                Response::from_data(bytes).with_header(header)
            }
            None => Response::from_string("not found").with_status_code(404),
        };
        let _ = request.respond(response);
    }
}
