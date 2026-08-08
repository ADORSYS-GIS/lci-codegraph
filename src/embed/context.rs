//! Builds the graph-aware text actually sent to the embedding model: a small deterministic header
//! (enclosing container, callees, callers) ahead of the chunk's own source. Pure, no I/O — this is
//! where most of this module's test coverage lives, since [`client`](super::client) needs a live (or
//! stubbed) endpoint to exercise and this doesn't.
//!
//! Determinism is a hard requirement here: the same walk, run twice, must produce byte-identical
//! embed inputs (an index rebuilt from an unchanged checkout should not dirty every vector). Every
//! `HashMap` built in this module is therefore read back through an explicit sort before it ever
//! reaches rendered output — insertion order (which follows `Graph`'s edge order, itself already
//! sorted by [`crate::graph::resolve`]) is never relied on directly.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::chunk::Chunk;
use crate::graph::{Graph, GraphNode};

use super::EmbedConfig;

/// A graph-derived index over one resolved [`Graph`], built once per walk (never per chunk — walking
/// `graph.edges` for every chunk would be O(chunks × edges)). Borrows the graph rather than cloning
/// it: a repo-sized graph can hold tens of thousands of nodes, and every value here is a reference
/// back into `graph`'s own storage.
pub(crate) struct ContextIndex<'g> {
    /// Nodes grouped by source file, each tagged with its 1-based `start_line`, for chunk→node
    /// mapping. Not keyed by `(file, line)` directly: a `HashMap` keyed on a borrowed tuple can only
    /// ever be looked up with a key borrowed for the *same* lifetime as the map's own borrow of
    /// `graph`, which a short-lived `&Chunk` can't satisfy — grouping by file (looked up through the
    /// stdlib's `&str: Borrow<str>` blanket impl, lifetime-independent) and filtering by line inside
    /// [`Self::map_chunk`] sidesteps that without cloning every file path.
    by_location: HashMap<&'g str, Vec<(i64, &'g GraphNode)>>,
    /// `calls` edges, source node id → callee nodes.
    callees: HashMap<&'g str, Vec<&'g GraphNode>>,
    /// `calls` edges, target node id → caller nodes.
    callers: HashMap<&'g str, Vec<&'g GraphNode>>,
    /// `contains`/`method` edges, target node id → its container node. The file→top-level-def edge is
    /// deliberately excluded (see [`Self::build`]) — it carries no information the header's `file:`
    /// line doesn't already give.
    container: HashMap<&'g str, &'g GraphNode>,
}

impl<'g> ContextIndex<'g> {
    /// Build the index from a resolved [`Graph`]. `O(nodes + edges)`.
    pub(crate) fn build(graph: &'g Graph) -> Self {
        let by_id: HashMap<&'g str, &'g GraphNode> = graph
            .nodes
            .iter()
            .map(|n| (n.node_id.as_str(), n))
            .collect();

        let mut by_location: HashMap<&'g str, Vec<(i64, &'g GraphNode)>> = HashMap::new();
        for n in &graph.nodes {
            by_location
                .entry(n.source_file.as_str())
                .or_default()
                .push((n.start_line, n));
        }

        let mut callees: HashMap<&'g str, Vec<&'g GraphNode>> = HashMap::new();
        let mut callers: HashMap<&'g str, Vec<&'g GraphNode>> = HashMap::new();
        let mut container: HashMap<&'g str, &'g GraphNode> = HashMap::new();

        for edge in &graph.edges {
            let (Some(&source), Some(&target)) = (
                by_id.get(edge.source.as_str()),
                by_id.get(edge.target.as_str()),
            ) else {
                // An edge whose endpoint isn't in `graph.nodes` shouldn't happen (the resolver only
                // ever emits edges between nodes it also emitted), but this index has no business
                // panicking over a graph invariant it doesn't own — skip rather than unwrap.
                continue;
            };
            match edge.relation.as_str() {
                "calls" => {
                    callees
                        .entry(source.node_id.as_str())
                        .or_default()
                        .push(target);
                    callers
                        .entry(target.node_id.as_str())
                        .or_default()
                        .push(source);
                }
                // Skip the file-node-as-container case (parent IS the file the child lives in): the
                // header's own `file:` line already says that.
                "contains" | "method" if source.node_id.as_str() != target.source_file.as_str() => {
                    container.insert(target.node_id.as_str(), source);
                }
                _ => {}
            }
        }

        Self {
            by_location,
            callees,
            callers,
            container,
        }
    }

    /// Map a chunk to its graph node. Graph node ids are `<file>#<line>:<name-or-kind>` with a
    /// **1-based** `start_line`, while [`Chunk::start_line`] is **0-based** — so the lookup is at
    /// `(chunk.file_path, chunk.start_line + 1)`. When several nodes share that location, the one
    /// whose `node_id` ends with `:{symbol_name}` wins when the chunk carries a symbol name;
    /// otherwise the first in sorted-`node_id` order, so the choice is deterministic rather than
    /// dependent on `Graph`'s incidental node order.
    ///
    /// Returns `None` for a `window` chunk, a PDF chunk, or any chunk from a walk with
    /// `build_graph: false` — that's the normal case, not an error.
    pub(crate) fn map_chunk(&self, chunk: &Chunk) -> Option<&'g GraphNode> {
        let line = i64::from(chunk.start_line) + 1;
        let candidates = self.by_location.get(chunk.file_path.as_str())?;
        let mut at_line: Vec<&'g GraphNode> = candidates
            .iter()
            .filter(|(l, _)| *l == line)
            .map(|(_, n)| *n)
            .collect();
        if at_line.is_empty() {
            return None;
        }
        if let Some(symbol_name) = &chunk.symbol_name {
            let suffix = format!(":{symbol_name}");
            if let Some(&exact) = at_line.iter().find(|n| n.node_id.ends_with(&suffix)) {
                return Some(exact);
            }
        }
        at_line.sort_by(|a, b| a.node_id.cmp(&b.node_id));
        Some(at_line[0])
    }

    fn container(&self, node: &GraphNode) -> Option<&'g GraphNode> {
        self.container.get(node.node_id.as_str()).copied()
    }

    fn callees(&self, node: &GraphNode) -> &[&'g GraphNode] {
        self.callees
            .get(node.node_id.as_str())
            .map_or(&[], Vec::as_slice)
    }

    fn callers(&self, node: &GraphNode) -> &[&'g GraphNode] {
        self.callers
            .get(node.node_id.as_str())
            .map_or(&[], Vec::as_slice)
    }
}

/// Render `nodes` as sorted, deduplicated `label [source_file]` entries, comma-joined, capped at
/// `cap` entries with a trailing `(+N more)` when truncated. `None` when `nodes` is empty (the caller
/// omits the whole header line rather than emitting an empty `calls:`/`called by:`).
fn render_refs(nodes: &[&GraphNode], cap: usize) -> Option<String> {
    if nodes.is_empty() {
        return None;
    }
    let mut rendered: Vec<String> = nodes
        .iter()
        .map(|n| format!("{} [{}]", n.label, n.source_file))
        .collect();
    rendered.sort();
    rendered.dedup();

    let total = rendered.len();
    if total > cap {
        let mut out = rendered[..cap].join(", ");
        let _ = write!(out, " (+{} more)", total - cap);
        Some(out)
    } else {
        Some(rendered.join(", "))
    }
}

/// Build the text actually sent to the embedding model for `chunk`: a header block, a blank line,
/// then the chunk content — truncated to `config.max_input_chars` **chars** (see
/// [`truncate_header_and_content`]).
///
/// `//` is used as the header's comment prefix for every source language, deliberately: this header
/// is never compiled, only read by an embedding model, so a per-language comment token
/// (`#`/`//`/`--`) would add a language dispatch table for zero retrieval benefit.
pub(crate) fn embed_input(chunk: &Chunk, index: &ContextIndex<'_>, config: &EmbedConfig) -> String {
    let mut header = String::new();
    let _ = writeln!(header, "// file: {}", chunk.file_path);
    let _ = writeln!(header, "// language: {}", chunk.language);
    match &chunk.symbol_name {
        Some(name) => {
            let _ = writeln!(header, "// {}: {name}", chunk.chunk_type);
        }
        None => {
            let _ = writeln!(header, "// {}", chunk.chunk_type);
        }
    }

    if let Some(node) = index.map_chunk(chunk) {
        if let Some(container) = index.container(node) {
            let _ = writeln!(header, "// within: {}", container.label);
        }
        if let Some(refs) = render_refs(index.callees(node), config.max_context_refs) {
            let _ = writeln!(header, "// calls: {refs}");
        }
        if let Some(refs) = render_refs(index.callers(node), config.max_context_refs) {
            let _ = writeln!(header, "// called by: {refs}");
        }
    }
    header.push('\n'); // blank line separating the header from the content

    truncate_header_and_content(&header, &chunk.content, config.max_input_chars)
}

/// Truncate `header + content` to `max_chars` **chars**, never splitting a `char` (so this can never
/// panic on a byte-slice boundary, unlike a naive `s[..n]`). The header is preserved and `content` is
/// truncated to whatever chars remain; only when the header alone is at or over the cap is the header
/// itself cut instead.
fn truncate_header_and_content(header: &str, content: &str, max_chars: usize) -> String {
    let header_chars = header.chars().count();
    if header_chars >= max_chars {
        return header.chars().take(max_chars).collect();
    }
    let mut out = String::with_capacity(header.len());
    out.push_str(header);
    out.extend(content.chars().take(max_chars - header_chars));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::GraphEdge;

    fn node(id: &str, label: &str, file: &str, line: i64) -> GraphNode {
        GraphNode {
            node_id: id.to_string(),
            label: label.to_string(),
            source_file: file.to_string(),
            start_line: line,
        }
    }

    fn edge(source: &str, target: &str, relation: &str) -> GraphEdge {
        GraphEdge {
            source: source.to_string(),
            target: target.to_string(),
            relation: relation.to_string(),
        }
    }

    fn chunk(file: &str, start_line: i32, symbol_name: Option<&str>, chunk_type: &str) -> Chunk {
        Chunk {
            file_path: file.to_string(),
            language: "rust".to_string(),
            chunk_type: chunk_type.to_string(),
            symbol_name: symbol_name.map(str::to_string),
            start_line,
            end_line: start_line,
            content: "fn body() {}".to_string(),
            embedding: None,
            embed_input: None,
        }
    }

    /// A small cross-file graph: `a.rs` defines `caller` (inside `impl Svc`) which calls `target` in
    /// `b.rs`; `target` also calls a `helper` that resolves to nothing (dangling on purpose, to prove
    /// unmapped callees are simply absent rather than crashing anything).
    fn sample_graph() -> Graph {
        Graph {
            nodes: vec![
                node("a.rs", "a.rs", "a.rs", 1),
                node("a.rs#2:Svc", "impl Svc", "a.rs", 2),
                node("a.rs#3:caller", "caller()", "a.rs", 3),
                node("b.rs", "b.rs", "b.rs", 1),
                node("b.rs#1:target", "target()", "b.rs", 1),
            ],
            edges: vec![
                edge("a.rs", "a.rs#2:Svc", "contains"),
                edge("a.rs#2:Svc", "a.rs#3:caller", "method"),
                edge("a.rs#3:caller", "b.rs#1:target", "calls"),
            ],
        }
    }

    #[test]
    fn maps_chunk_to_node_via_0_based_to_1_based_offset() {
        let graph = sample_graph();
        let index = ContextIndex::build(&graph);
        // Chunk::start_line is 0-based; the node at a.rs line 3 (1-based) maps from chunk start_line 2.
        let c = chunk("a.rs", 2, Some("caller"), "function");
        let mapped = index.map_chunk(&c).expect("chunk should map to a node");
        assert_eq!(mapped.node_id, "a.rs#3:caller");
    }

    #[test]
    fn within_line_comes_from_the_method_edge_container() {
        let graph = sample_graph();
        let index = ContextIndex::build(&graph);
        let config = EmbedConfig::builder().base_url("http://x").build();
        let c = chunk("a.rs", 2, Some("caller"), "function");
        let input = embed_input(&c, &index, &config);
        assert!(
            input.contains("// within: impl Svc"),
            "expected a within: line, got:\n{input}"
        );
    }

    #[test]
    fn calls_and_called_by_render_sorted_deduped_and_capped() {
        let mut graph = sample_graph();
        // Add a second, duplicate-label callee and a third distinct one so sorting + dedup + the cap
        // all have something to do.
        graph.nodes.push(node("b.rs#2:zzz", "zzz()", "b.rs", 2));
        graph.nodes.push(node("b.rs#3:aaa", "aaa()", "b.rs", 3));
        graph
            .edges
            .push(edge("a.rs#3:caller", "b.rs#2:zzz", "calls"));
        graph
            .edges
            .push(edge("a.rs#3:caller", "b.rs#3:aaa", "calls"));
        // A duplicate call to the same target must dedup to one rendered entry.
        graph
            .edges
            .push(edge("a.rs#3:caller", "b.rs#1:target", "calls"));

        let index = ContextIndex::build(&graph);
        let config = EmbedConfig::builder()
            .base_url("http://x")
            .max_context_refs(2)
            .build();
        let c = chunk("a.rs", 2, Some("caller"), "function");
        let input = embed_input(&c, &index, &config);

        let calls_line = input
            .lines()
            .find(|l| l.starts_with("// calls:"))
            .expect("expected a calls: line");
        // Sorted alphabetically: aaa(), target(), zzz() [src b.rs] — capped to 2, "+1 more".
        assert_eq!(
            calls_line,
            "// calls: aaa() [b.rs], target() [b.rs] (+1 more)"
        );

        let target_index = ContextIndex::build(&graph);
        let target_chunk = chunk("b.rs", 0, Some("target"), "function");
        let target_input = embed_input(&target_chunk, &target_index, &config);
        assert!(
            target_input.contains("// called by: caller() [a.rs]"),
            "expected a called-by line naming caller(), got:\n{target_input}"
        );
    }

    #[test]
    fn unmapped_window_chunk_gets_a_header_with_no_graph_lines() {
        let graph = sample_graph();
        let index = ContextIndex::build(&graph);
        let config = EmbedConfig::builder().base_url("http://x").build();
        // A window chunk: no symbol name, and a line with no node at all (line 99).
        let c = chunk("a.rs", 99, None, "window");
        let input = embed_input(&c, &index, &config);
        assert!(input.starts_with("// file: a.rs\n// language: rust\n// window\n\n"));
        assert!(!input.contains("within:"));
        assert!(!input.contains("calls:"));
        assert!(!input.contains("called by:"));
    }

    #[test]
    fn build_is_deterministic_across_two_runs_over_the_same_graph() {
        let graph = sample_graph();
        let config = EmbedConfig::builder().base_url("http://x").build();
        let c = chunk("a.rs", 2, Some("caller"), "function");

        let first = embed_input(&c, &ContextIndex::build(&graph), &config);
        let second = embed_input(&c, &ContextIndex::build(&graph), &config);
        assert_eq!(
            first, second,
            "the same chunk + graph must render byte-identical input"
        );
    }

    #[test]
    fn truncation_never_splits_a_multi_byte_char() {
        // "café 🎉" — 'é' and '🎉' are multi-byte in UTF-8 but each one `char`; a byte-slice
        // truncation landing mid-character would panic or produce invalid UTF-8 (`String` can't hold
        // either, so the test not panicking is itself part of the assertion). A char-based cap must
        // land cleanly regardless of where it falls relative to those boundaries.
        let header = "// file: f.rs\n// language: rust\n// window\n\n";
        let content = "café 🎉 more text than fits";
        let header_chars = header.chars().count();
        for cap in [0, 1, header_chars, header_chars + 3, 1000] {
            let out = truncate_header_and_content(header, content, cap);
            assert!(
                out.chars().count() <= cap,
                "cap {cap}: output has {} chars",
                out.chars().count()
            );
        }
        // A cap comfortably past the header must actually include some content, not just the header.
        let out = truncate_header_and_content(header, content, header_chars + 3);
        assert!(out.starts_with(header));
        assert_eq!(&out[header.len()..], "caf");
    }

    #[test]
    fn header_only_is_truncated_when_it_alone_exceeds_the_cap() {
        let header =
            "// file: a-very-long-file-name-that-is-quite-verbose.rs\n// language: rust\n\n";
        let content = "short";
        let cap = 10;
        let out = truncate_header_and_content(header, content, cap);
        assert_eq!(out.chars().count(), cap);
        assert!(
            !out.contains("short"),
            "content must be entirely dropped once the header alone exceeds the cap"
        );
    }

    #[test]
    fn missing_symbol_name_falls_back_to_bare_chunk_type_line() {
        let graph = Graph::default();
        let index = ContextIndex::build(&graph);
        let config = EmbedConfig::builder().base_url("http://x").build();
        let c = chunk("f.rs", 0, None, "impl");
        let input = embed_input(&c, &index, &config);
        assert!(input.contains("// impl\n"));
        assert!(!input.contains("// impl:"));
    }
}
