//! HTML renderer over `tfmx_analysis::SongView` (`docs/m5-plan.md` Phase
//! 5.8). Waveform SVG and trackstep table are inline, no dependency either
//! way. The Mermaid call graph gets two tabs: "Diagram" tries to load
//! Mermaid.js from a CDN at view time and render it, falling back to a
//! plain message if that fetch fails (no internet at open time); "Source"
//! always shows the raw Mermaid text, so the graph is never unreadable.

use tfmx_analysis::{SongView, StepView, TrackSlotView};

use crate::mermaid::call_graph_to_mermaid;

const SVG_WIDTH: f64 = 800.0;
const ROW_HEIGHT: f64 = 18.0;

fn waveform_svg(view: &SongView) -> String {
    let smpl_len = view.waveform.smpl_len.max(1) as f64;
    let macros: Vec<u8> = {
        let mut m: Vec<u8> = view
            .waveform
            .regions
            .iter()
            .map(|r| r.macro_number)
            .collect();
        m.sort_unstable();
        m.dedup();
        m
    };
    let height = (macros.len() as f64 * ROW_HEIGHT).max(ROW_HEIGHT);

    let mut svg = format!(
        "<svg width=\"{SVG_WIDTH}\" height=\"{height}\" viewBox=\"0 0 {SVG_WIDTH} {height}\">\n"
    );
    for (row, &macro_number) in macros.iter().enumerate() {
        let y = row as f64 * ROW_HEIGHT;
        for region in view
            .waveform
            .regions
            .iter()
            .filter(|r| r.macro_number == macro_number)
        {
            let x = (region.start as f64 / smpl_len * SVG_WIDTH).min(SVG_WIDTH);
            // Not clamped to the remaining canvas width -- an out-of-bounds
            // region reading past `smpl_len` is expected to overflow the
            // waveform's own axis; that's what makes it visible as "past
            // the end", not a rendering bug to paper over.
            let w = (region.len as f64 / smpl_len * SVG_WIDTH).max(1.0);
            let class = if region.out_of_bounds {
                "region oob"
            } else {
                "region"
            };
            svg.push_str(&format!(
                "  <rect class=\"{class}\" x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{:.1}\">\
                 <title>macro {macro_number}: {} bytes at {} ({}{})</title></rect>\n",
                ROW_HEIGHT - 2.0,
                region.len,
                region.start,
                if region.looped { "looped" } else { "one-shot" },
                if region.out_of_bounds {
                    ", OUT OF BOUNDS"
                } else {
                    ""
                },
            ));
        }
    }
    svg.push_str("</svg>\n");
    svg
}

/// Escapes text content for embedding inside an HTML `<pre>` element.
/// Attribute-only characters (quotes) are left alone; this crate's own
/// Mermaid/table cell text never carries attributes.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn track_slot_cell(slot: TrackSlotView) -> String {
    match slot {
        TrackSlotView::Pattern { number, transpose } => format!("P{number} ({transpose:+})"),
        TrackSlotView::Hold { transpose } => format!("hold ({transpose:+})"),
        TrackSlotView::StopChannel => "stop".to_string(),
        TrackSlotView::StopVoice { voice } => format!("stop v{voice}"),
    }
}

fn trackstep_table(view: &SongView) -> String {
    let mut table = String::from("<table class=\"trackstep\">\n<tr><th>line</th>");
    for t in 0..8 {
        table.push_str(&format!("<th>track {t}</th>"));
    }
    table.push_str("</tr>\n");
    for step in &view.trackstep.steps {
        table.push_str(&format!("<tr><td>{}</td>", step.line));
        match &step.step {
            StepView::Tracks(slots) => {
                for slot in slots {
                    table.push_str(&format!("<td>{}</td>", track_slot_cell(*slot)));
                }
            }
            StepView::Command(cmd) => {
                table.push_str(&format!("<td colspan=\"8\">{cmd:?}</td>"));
            }
        }
        table.push_str("</tr>\n");
    }
    table.push_str("</table>\n");
    table
}

const STYLE: &str = "
body { font: 14px sans-serif; margin: 2rem; }
.region { fill: #4a90d9; stroke: #2c5a8a; }
.region.oob { fill: #d94a4a; stroke: #8a2c2c; }
table.trackstep { border-collapse: collapse; font-size: 12px; }
table.trackstep td, table.trackstep th { border: 1px solid #ccc; padding: 2px 6px; }
pre.mermaid-source { background: #f4f4f4; padding: 1rem; overflow-x: auto; }
.tabs { margin: 0.5rem 0 0; }
.tab-button { font: inherit; padding: 0.3rem 0.9rem; border: 1px solid #ccc; border-bottom: none;
  background: #eee; cursor: pointer; }
.tab-button.active { background: #fff; font-weight: bold; }
.tab-panel { border: 1px solid #ccc; padding: 1rem; }
.tab-panel[hidden] { display: none; }
#mermaid-offline-note { color: #8a2c2c; }
";

/// The tabbed "Diagram"/"Source" pair over the call graph: "Diagram" tries
/// to load Mermaid.js from a CDN at view time and render `graph`, falling
/// back to `#mermaid-offline-note` if that script fails to load (no
/// internet at open time); "Source" always shows `graph`'s raw text.
const GRAPH_SCRIPT: &str = "
(function () {
  var tabDiagram = document.getElementById('tab-diagram');
  var tabSource = document.getElementById('tab-source');
  var panelDiagram = document.getElementById('panel-diagram');
  var panelSource = document.getElementById('panel-source');
  function show(tab) {
    var diagram = tab === 'diagram';
    tabDiagram.classList.toggle('active', diagram);
    tabSource.classList.toggle('active', !diagram);
    panelDiagram.hidden = !diagram;
    panelSource.hidden = diagram;
  }
  tabDiagram.addEventListener('click', function () { show('diagram'); });
  tabSource.addEventListener('click', function () { show('source'); });

  var script = document.createElement('script');
  script.src = 'https://cdn.jsdelivr.net/npm/mermaid@10/dist/mermaid.min.js';
  script.onload = function () { mermaid.initialize({ startOnLoad: true }); };
  script.onerror = function () {
    document.getElementById('mermaid-offline-note').hidden = false;
  };
  document.head.appendChild(script);
})();
";

/// Renders `view` (already built via `tfmx_analysis::build_song_view`) as a
/// single HTML page: waveform regions (out-of-bounds ones visibly marked in
/// red), the pattern->macro call graph (a rendered diagram when the page
/// can reach a CDN, its Mermaid source always), and the trackstep structure
/// map.
pub fn render_html(module_name: &str, view: &SongView) -> String {
    let graph = escape_html(&call_graph_to_mermaid(&view.walk));
    format!(
        "<!doctype html>\n<html><head><meta charset=\"utf-8\">\n\
         <title>{module_name} -- song {song}</title>\n\
         <style>{STYLE}</style>\n</head><body>\n\
         <h1>{module_name} -- song {song}</h1>\n\
         <h2>Waveform regions</h2>\n\
         <p>smpl length: {smpl_len} bytes; red = out of bounds</p>\n\
         {svg}\
         <h2>Pattern &rarr; macro call graph</h2>\n\
         <div class=\"tabs\">\
         <button type=\"button\" id=\"tab-diagram\" class=\"tab-button active\">Diagram</button>\
         <button type=\"button\" id=\"tab-source\" class=\"tab-button\">Source</button>\
         </div>\n\
         <div id=\"panel-diagram\" class=\"tab-panel\">\
         <pre class=\"mermaid\">{graph}</pre>\
         <p id=\"mermaid-offline-note\" hidden>Rendering the diagram needs internet access to \
         load Mermaid.js from a CDN. Showing the raw diagram source instead -- see the Source \
         tab.</p>\
         </div>\n\
         <div id=\"panel-source\" class=\"tab-panel\" hidden><pre class=\"mermaid-source\">{graph}</pre></div>\n\
         <h2>Trackstep structure</h2>\n\
         {table}\
         <script>{GRAPH_SCRIPT}</script>\n\
         </body></html>\n",
        song = view.song,
        smpl_len = view.waveform.smpl_len,
        svg = waveform_svg(view),
        table = trackstep_table(view),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use tfmx_analysis::{TrackstepMap, TrackstepStep, WaveformRegion, WaveformView};

    fn view_with_regions(regions: Vec<WaveformRegion>, smpl_len: u32) -> SongView {
        SongView {
            song: 0,
            waveform: WaveformView { smpl_len, regions },
            walk: tfmx_analysis::WalkResult {
                reachable_patterns: BTreeSet::from([1]),
                reachable_macros: BTreeSet::from([9]),
                ..Default::default()
            },
            trackstep: TrackstepMap {
                steps: vec![TrackstepStep {
                    line: 0,
                    step: StepView::Tracks([
                        TrackSlotView::Pattern {
                            number: 1,
                            transpose: 0,
                        },
                        TrackSlotView::StopChannel,
                        TrackSlotView::StopChannel,
                        TrackSlotView::StopChannel,
                        TrackSlotView::StopChannel,
                        TrackSlotView::StopChannel,
                        TrackSlotView::StopChannel,
                        TrackSlotView::StopChannel,
                    ]),
                }],
            },
        }
    }

    #[test]
    fn out_of_bounds_region_is_visibly_marked() {
        let view = view_with_regions(
            vec![WaveformRegion {
                macro_number: 0,
                start: 0,
                len: 32,
                looped: false,
                out_of_bounds: true,
            }],
            8,
        );

        let html = render_html("probe", &view);

        assert!(html.contains("class=\"region oob\""));
        assert!(html.contains("OUT OF BOUNDS"));
    }

    #[test]
    fn in_bounds_region_has_no_oob_marker() {
        let view = view_with_regions(
            vec![WaveformRegion {
                macro_number: 0,
                start: 0,
                len: 4,
                looped: false,
                out_of_bounds: false,
            }],
            8,
        );

        let html = render_html("probe", &view);

        assert!(html.contains("class=\"region\""));
        assert!(!html.contains("oob\""));
        assert!(!html.contains("OUT OF BOUNDS"));
    }

    #[test]
    fn embeds_mermaid_source_and_trackstep_table() {
        let view = view_with_regions(vec![], 8);

        let html = render_html("probe", &view);

        assert!(html.contains("flowchart LR"));
        assert!(html.contains("P1 --> M9") || html.contains("P1[\"Pattern 1\"]"));
        assert!(html.contains("<table class=\"trackstep\">"));
        assert!(html.contains("P1 (+0)"));
    }

    #[test]
    fn graph_has_a_diagram_tab_that_tries_a_cdn_and_a_source_tab_with_the_offline_fallback() {
        let view = view_with_regions(vec![], 8);

        let html = render_html("probe", &view);

        // Two tabs, each with its own panel.
        assert!(html.contains("id=\"tab-diagram\""));
        assert!(html.contains("id=\"tab-source\""));
        assert!(html.contains("id=\"panel-diagram\""));
        assert!(html.contains("id=\"panel-source\""));
        // The diagram panel holds a `.mermaid` block Mermaid.js renders in
        // place, plus a fallback note that starts hidden.
        assert!(html.contains("<pre class=\"mermaid\">"));
        assert!(html.contains("id=\"mermaid-offline-note\" hidden"));
        // The source panel always has the raw text, unconditionally.
        assert!(html.contains("<pre class=\"mermaid-source\">"));
        // Mermaid.js is loaded dynamically (not a static <script src=...>),
        // so `onerror` can reveal the offline note instead of a broken page.
        assert!(html.contains("cdn.jsdelivr.net"));
        assert!(html.contains("script.onerror"));
    }

    #[test]
    fn is_well_formed_html() {
        let view = view_with_regions(vec![], 0);
        let html = render_html("probe", &view);
        assert!(html.starts_with("<!doctype html>"));
        assert!(html.trim_end().ends_with("</html>"));
    }
}
