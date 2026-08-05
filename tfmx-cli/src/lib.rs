//! Library surface exposing `visualize::render_html` so `tfmx-web-gui` can
//! reuse the existing SVG/Mermaid HTML renderer instead of duplicating it
//! (`docs/gui-plan.md` Phase W2). Everything else stays behind `main.rs`.

pub mod disasm_text;
mod mermaid;
pub mod visualize;
