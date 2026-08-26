//! Black-box tests for [`Chunk::node_id`] (issue #12): each chunk derived from a definition the graph
//! pass also found is linked to that definition's node id, computed and verified inside the crate from
//! the same per-file facts — never guessed by an external caller reconstructing a naming convention.
//!
//! Determinism (two walks of the same checkout produce identical `node_id` assignments) is already
//! covered by `tests/determinism.rs`'s `same_checkout_walked_twice_is_byte_identical`, which compares
//! `out1.chunks == out2.chunks` — `Chunk`'s derived `PartialEq` now includes `node_id`, so a
//! non-deterministic assignment would fail that test too. Nothing new is added here for it.

use lci_codegraph::{IndexOptions, RawInput, index_inputs};

#[test]
fn a_rust_function_chunk_is_linked_to_its_graph_node() {
    // Rust: the graph classifier IS `chunk::interesting_node` (the same function chunking uses), so
    // this is the case the issue itself measured as already working end-to-end (`shapes.rs`: 4/4
    // matched) — the sanity check that the join actually fires, not just compiles.
    let options = IndexOptions::builder().build_graph(true).build();
    let out = index_inputs(
        vec![RawInput::text(
            "src/math.rs",
            "fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )],
        &options,
    );

    let chunk = out
        .chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("add"))
        .expect("add() chunk produced");
    let node_id = chunk
        .node_id
        .as_deref()
        .expect("a chunk derived from a definition must be linked");

    assert!(
        out.graph.nodes.iter().any(|n| n.node_id == node_id),
        "linked node_id {node_id:?} must resolve to an actual node in graph.nodes: {:?}",
        out.graph.nodes
    );
    assert_eq!(node_id, "src/math.rs#1:add");
}

#[test]
fn a_tags_language_function_chunk_is_linked_to_its_graph_node() {
    // Not Rust: Python is classified via the grammar's `tags.scm` query (`GraphStrategy::Tags`), a
    // completely independent pass from `chunk::interesting_node` — the acceptance criterion this test
    // exists for ("not only Rust — Rust is the case that already works and would not catch a
    // regression here").
    let options = IndexOptions::builder().build_graph(true).build();
    let out = index_inputs(
        vec![RawInput::text(
            "svc/greet.py",
            "def greet():\n    return 'hi'\n",
        )],
        &options,
    );

    let chunk = out
        .chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("greet"))
        .expect("greet() chunk produced");
    let node_id = chunk
        .node_id
        .as_deref()
        .expect("a chunk derived from a tags-language definition must be linked too");

    assert!(
        out.graph.nodes.iter().any(|n| n.node_id == node_id),
        "linked node_id {node_id:?} must resolve to an actual node in graph.nodes: {:?}",
        out.graph.nodes
    );
}

#[test]
fn node_id_is_none_for_every_chunk_when_build_graph_is_false() {
    let options = IndexOptions::builder().build_graph(false).build();
    let out = index_inputs(
        vec![RawInput::text(
            "src/math.rs",
            "fn add(a: i32, b: i32) -> i32 { a + b }\n",
        )],
        &options,
    );

    assert!(!out.chunks.is_empty(), "chunks are still produced");
    assert!(
        out.chunks.iter().all(|c| c.node_id.is_none()),
        "no graph ran, so nothing can be linked: {:?}",
        out.chunks
    );
}

#[test]
fn node_id_is_none_for_a_windowed_fallback_chunk_even_with_build_graph_true() {
    // Valid Rust with a grammar, but nothing `interesting_node` classifies — falls back to windowed
    // chunking (`chunk_type: "window"`) even though `build_graph` is on. A windowed chunk never
    // corresponds 1:1 with a single definition, so it must never be linked.
    let options = IndexOptions::builder().build_graph(true).build();
    let out = index_inputs(
        vec![RawInput::text(
            "src/uses.rs",
            "use std::collections::HashMap;\n",
        )],
        &options,
    );

    assert!(
        !out.chunks.is_empty(),
        "must still fall back to windowed chunks"
    );
    assert!(out.chunks.iter().all(|c| c.chunk_type == "window"));
    assert!(
        out.chunks.iter().all(|c| c.node_id.is_none()),
        "a windowed chunk must never be linked to a node: {:?}",
        out.chunks
    );
}

#[test]
fn node_id_is_none_for_a_chunk_with_no_matching_definition() {
    // TypeScript: an arrow function bound to a `variable_declarator` is chunked ANONYMOUSLY
    // (`chunk::interesting_node`'s `variable_declarator` arm returns `None`, so `arrow_function`
    // yields `("function", None)` — no name reaches the chunk), while the tags-driven graph pass DOES
    // recover the binding's name. The computed candidate id (keyed on the chunk's own, name-less
    // fallback) therefore cannot exist in the real node set — documented behaviour (issue #11), not a
    // bug #12 introduces: `None` is the only honest answer here, not a guess.
    let options = IndexOptions::builder().build_graph(true).build();
    let out = index_inputs(
        vec![RawInput::text(
            "web/a.ts",
            "export const arrowFn = (a: number): number => a + 1;\n",
        )],
        &options,
    );

    assert!(
        out.graph.nodes.iter().any(|n| n.label == "arrowFn()"),
        "sanity: the graph pass does know this symbol by name: {:?}",
        out.graph.nodes
    );
    let chunk = out
        .chunks
        .iter()
        .find(|c| c.chunk_type == "function")
        .expect("the arrow function is still chunked, just anonymously");
    assert_eq!(chunk.symbol_name, None);
    assert_eq!(
        chunk.node_id, None,
        "no honest match exists for an anonymous chunk whose graph def DOES have a name"
    );
}
