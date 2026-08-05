//! Local browser GUI for TFMX tooling (`docs/gui-plan.md`). Serves the
//! static page plus the `Session`-backed API routes (`/files`, `/load`,
//! `/song-view`, `/disasm`, `/render-*`) Phase W1 adds; the real page layout
//! consuming them is Phase W2.

mod query;
mod routes;
mod session;
mod static_files;

use session::Session;
use tiny_http::{Method, Response, Server};

fn main() {
    let addr = "127.0.0.1:8080";
    let server = Server::http(addr).unwrap_or_else(|e| panic!("bind {addr}: {e}"));
    let static_dir = std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/static"));
    eprintln!("tfmx-web-gui listening on http://{addr}");

    let mut session: Option<Session> = None;

    for mut request in server.incoming_requests() {
        let path = request.url().split('?').next().unwrap_or("/").to_string();
        let query = query::parse(request.url());

        let response = match (request.method(), path.as_str()) {
            (Method::Get, "/files") => routes::list_files(&query),
            (Method::Post, "/load") => {
                let mut body = String::new();
                let _ = request.as_reader().read_to_string(&mut body);
                routes::load(&mut session, &body)
            }
            (Method::Get, "/disasm") => routes::disasm(session.as_ref(), &query),
            (Method::Get, "/disasm-text") => routes::disasm_text(session.as_ref(), &query),
            (Method::Get, "/module-info") => routes::module_info(session.as_ref(), &query),
            (Method::Get, "/render-macro") => routes::render_macro(session.as_ref(), &query),
            (Method::Get, "/render-pattern") => routes::render_pattern(session.as_ref(), &query),
            (Method::Get, "/render-region") => routes::render_region(session.as_ref(), &query),
            (Method::Get, "/song-view") => routes::song_view(session.as_ref(), &query),
            (Method::Get, "/song-view.html") => routes::song_view_html(session.as_ref(), &query),
            (Method::Get, _) => match static_files::resolve(&static_dir, request.url()) {
                Some(path) => {
                    let content_type = static_files::content_type(&path);
                    let bytes = std::fs::read(&path).unwrap_or_default();
                    let header = tiny_http::Header::from_bytes(
                        &b"Content-Type"[..],
                        content_type.as_bytes(),
                    )
                    .expect("static content-type is valid header bytes");
                    Response::from_data(bytes).with_header(header)
                }
                None => routes::not_found(),
            },
            _ => routes::not_found(),
        };
        let _ = request.respond(response);
    }
}
