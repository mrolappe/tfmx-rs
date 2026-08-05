//! Resolves a request path to a file under `static/`, rejecting anything
//! that escapes it (`..`, symlinks out) -- the server binds to localhost
//! only, but any page open in the same browser can still address it, so
//! path traversal is a real trust boundary, not a hypothetical one.

use std::path::{Path, PathBuf};

pub fn resolve(static_dir: &Path, url_path: &str) -> Option<PathBuf> {
    let relative = url_path.split('?').next().unwrap_or("/");
    let relative = if relative == "/" {
        "index.html"
    } else {
        relative.trim_start_matches('/')
    };
    let candidate = static_dir.join(relative);
    let canonical = candidate.canonicalize().ok()?;
    let static_dir = static_dir.canonicalize().ok()?;
    canonical.starts_with(&static_dir).then_some(canonical)
}

pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn static_dir() -> PathBuf {
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/static"))
    }

    #[test]
    fn root_resolves_to_index_html() {
        let resolved = resolve(&static_dir(), "/").expect("index.html exists");
        assert_eq!(resolved.file_name().unwrap(), "index.html");
    }

    #[test]
    fn a_query_string_is_stripped_before_resolving() {
        let resolved = resolve(&static_dir(), "/index.html?foo=bar").expect("index.html exists");
        assert_eq!(resolved.file_name().unwrap(), "index.html");
    }

    #[test]
    fn path_traversal_out_of_static_dir_is_rejected() {
        assert_eq!(resolve(&static_dir(), "/../Cargo.toml"), None);
        assert_eq!(resolve(&static_dir(), "/../../Cargo.toml"), None);
    }

    #[test]
    fn a_missing_file_is_rejected() {
        assert_eq!(resolve(&static_dir(), "/does-not-exist.html"), None);
    }

    #[test]
    fn content_type_is_derived_from_extension() {
        assert_eq!(
            content_type(Path::new("index.html")),
            "text/html; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("app.js")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(
            content_type(Path::new("data.bin")),
            "application/octet-stream"
        );
    }
}
