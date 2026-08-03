//! Pure string building: a `WalkResult`'s reachable patterns/macros and
//! edges as a Mermaid flowchart. `docs/m5-plan.md` Phase 5.8.

use tfmx_analysis::{SpanKind, WalkResult};

fn node_id(kind: SpanKind) -> String {
    match kind {
        SpanKind::Pattern(n) => format!("P{n}"),
        SpanKind::Macro(n) => format!("M{n}"),
    }
}

fn node_label(kind: SpanKind) -> String {
    match kind {
        SpanKind::Pattern(n) => format!("P{n}[\"Pattern {n}\"]"),
        SpanKind::Macro(n) => format!("M{n}((\"Macro {n}\"))"),
    }
}

/// Renders `walk`'s reachable patterns/macros and edges as a Mermaid
/// `flowchart` block (patterns as boxes, macros as circles). Patterns and
/// macros are disjoint id namespaces (`P<n>`/`M<n>`), so a pattern and a
/// macro sharing a number never collide.
pub fn call_graph_to_mermaid(walk: &WalkResult) -> String {
    let mut out = String::from("flowchart LR\n");
    for &n in &walk.reachable_patterns {
        out.push_str(&format!("    {}\n", node_label(SpanKind::Pattern(n))));
    }
    for &n in &walk.reachable_macros {
        out.push_str(&format!("    {}\n", node_label(SpanKind::Macro(n))));
    }
    for edge in &walk.edges {
        out.push_str(&format!(
            "    {} --> {}\n",
            node_id(edge.from),
            node_id(edge.to)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use tfmx_analysis::Edge;

    #[test]
    fn renders_pattern_and_macro_nodes_with_edges() {
        let walk = WalkResult {
            reachable_patterns: BTreeSet::from([1]),
            reachable_macros: BTreeSet::from([9]),
            edges: vec![Edge {
                from: SpanKind::Pattern(1),
                to: SpanKind::Macro(9),
            }],
            ..WalkResult::default()
        };

        let mermaid = call_graph_to_mermaid(&walk);

        assert!(mermaid.starts_with("flowchart LR\n"));
        assert!(mermaid.contains("P1[\"Pattern 1\"]"));
        assert!(mermaid.contains("M9((\"Macro 9\"))"));
        assert!(mermaid.contains("P1 --> M9"));
    }

    #[test]
    fn pattern_and_macro_with_the_same_number_get_distinct_ids() {
        let walk = WalkResult {
            reachable_patterns: BTreeSet::from([5]),
            reachable_macros: BTreeSet::from([5]),
            edges: vec![Edge {
                from: SpanKind::Pattern(5),
                to: SpanKind::Macro(5),
            }],
            ..WalkResult::default()
        };

        let mermaid = call_graph_to_mermaid(&walk);

        assert!(mermaid.contains("P5 --> M5"));
    }

    #[test]
    fn empty_walk_renders_just_the_header() {
        let mermaid = call_graph_to_mermaid(&WalkResult::default());
        assert_eq!(mermaid, "flowchart LR\n");
    }
}
