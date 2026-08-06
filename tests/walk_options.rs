//! Black-box tests over [`WalkOptions`] / [`walk_checkout`]'s top-level knobs: `build_graph` gating
//! chunks vs. graph, and the `WalkOptions` builder's documented defaults.

mod common;

use lci_codegraph::{WalkOptions, walk_checkout};

#[test]
fn build_graph_false_yields_chunks_but_empty_graph() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn caller() { target(); }\n");
    common::write(root, "src/b.rs", "fn target() {}\n");

    let options = WalkOptions::builder().build_graph(false).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(!out.chunks.is_empty(), "chunks are still produced");
    assert!(out.graph.nodes.is_empty(), "graph off ⇒ no nodes");
    assert!(out.graph.edges.is_empty(), "graph off ⇒ no edges");
}

#[test]
fn build_graph_true_yields_chunks_and_graph() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn caller() { target(); }\n");
    common::write(root, "src/b.rs", "fn target() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(!out.chunks.is_empty(), "chunks are produced");
    assert!(!out.graph.nodes.is_empty(), "graph nodes are produced");
    assert!(
        out.graph.edges.iter().any(|e| e.relation == "calls"),
        "cross-file calls edge; edges = {:?}",
        out.graph.edges
    );
}

#[test]
fn default_walk_options_are_graph_off_gitignore_on_pdfs_on() {
    // Undocumented behaviour change here would be a silent breaking change for every existing host —
    // pin the builder defaults exactly as `src/walk.rs` documents them.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn a() {}\n");
    common::write(root, ".gitignore", "ignored.rs\n");
    common::write(root, "ignored.rs", "fn should_not_be_walked() {}\n");

    let out = walk_checkout(root, &WalkOptions::builder().build()).unwrap();

    assert!(
        out.graph.nodes.is_empty(),
        "build_graph defaults to false (behaviour-neutral until a host opts in)"
    );
    assert!(
        !out.chunks
            .iter()
            .any(|c| c.file_path.contains("ignored.rs")),
        "respect_gitignore defaults to true"
    );
    assert!(
        out.chunks.iter().any(|c| c.file_path == "src/a.rs"),
        "non-ignored source is still chunked"
    );
}

#[test]
fn a_language_with_no_graph_extractor_is_still_chunked_but_contributes_no_graph_nodes() {
    // `go`/`c`/`text` etc. have no graph strategy — the walk must not silently drop them from
    // chunking just because `build_graph` is on for the rest of the repo.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn a() {}\n");
    common::write(root, "cmd/main.go", "package main\n\nfunc main() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        out.chunks.iter().any(|c| c.file_path == "cmd/main.go"),
        "go file is still chunked; chunks = {:?}",
        out.chunks
    );
    assert!(
        !out.graph
            .nodes
            .iter()
            .any(|n| n.source_file == "cmd/main.go"),
        "go has no graph extractor, so it contributes no graph nodes; nodes = {:?}",
        out.graph.nodes
    );
}
