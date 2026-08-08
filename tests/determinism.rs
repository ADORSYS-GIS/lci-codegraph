//! Determinism guarantees `walk_checkout` must uphold (ADR-0086): the structural graph is
//! canonicalised (sorted + deduped, see `graph::resolve`) *specifically* so it's stable to snapshot
//! as a golden and stable to submit — this suite is the black-box proof of that property through the
//! public API, not just the `graph` module's own internal unit test
//! (`graph::tests::output_is_deterministic`).

mod common;

use std::path::{Path, PathBuf};

use lci_codegraph::{
    EmbedConfig, IndexOptions, RawInput, WalkOptions, index_inputs, walk_checkout,
};

#[test]
fn same_checkout_walked_twice_is_byte_identical() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn caller() { target(); }\n");
    common::write(root, "src/b.rs", "fn target() {}\n");
    common::write(root, "src/c.rs", "struct S;\nimpl S { fn run(&self) {} }\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out1 = walk_checkout(root, &options).unwrap();
    let out2 = walk_checkout(root, &options).unwrap();

    let json1 = serde_json::to_string_pretty(&out1.graph).unwrap();
    let json2 = serde_json::to_string_pretty(&out2.graph).unwrap();
    assert_eq!(json1, json2, "serialised graph must be byte-identical");
    assert_eq!(out1.graph, out2.graph);

    assert_eq!(
        out1.chunks, out2.chunks,
        "chunks must be identically ordered across repeat walks of the same checkout"
    );
}

#[test]
fn graph_does_not_depend_on_filesystem_iteration_order() {
    // Two checkouts with the SAME file set + content, but the files are created on disk in opposite
    // order. If `resolve` (or anything upstream of it) accidentally depended on discovery order —
    // e.g. "first definition wins" instead of "count candidates, only emit on a clean single match" —
    // this would catch it: three files each define `dup`, so a bare `dup()` call is genuinely
    // ambiguous regardless of which definition the walker happens to see first.
    let files: &[(&str, &str)] = &[
        ("src/x.rs", "fn dup() {}\n"),
        ("src/y.rs", "fn dup() {}\n"),
        ("src/z.rs", "fn caller() { dup(); }\n"),
        ("src/a.rs", "fn helper() {}\nfn user() { helper(); }\n"),
    ];

    let forward = tempfile::tempdir().unwrap();
    for (path, body) in files {
        common::write(forward.path(), path, body);
    }

    let reverse = tempfile::tempdir().unwrap();
    for (path, body) in files.iter().rev() {
        common::write(reverse.path(), path, body);
    }

    let options = WalkOptions::builder().build_graph(true).build();
    let g1 = walk_checkout(forward.path(), &options).unwrap().graph;
    let g2 = walk_checkout(reverse.path(), &options).unwrap().graph;

    assert_eq!(
        g1, g2,
        "canonicalised graph must be identical regardless of on-disk creation order"
    );
    // Sanity: the ambiguous `dup()` call really did produce no guessed edge in either run.
    assert!(
        !g1.edges
            .iter()
            .any(|e| e.relation == "calls" && e.target.contains("dup")),
        "ambiguous dup() must never resolve; edges = {:?}",
        g1.edges
    );
    assert!(
        g1.edges.iter().any(|e| e.relation == "calls"
            && e.source.contains("user")
            && e.target.contains("helper")),
        "the unambiguous helper() call must still resolve; edges = {:?}",
        g1.edges
    );
}

#[test]
fn embed_inputs_are_byte_identical_across_two_walks() {
    // `embed::context::embed_input` only runs when `embed` is configured, so this needs a live (if
    // stubbed) endpoint — unlike `same_checkout_walked_twice_is_byte_identical` above, which never
    // dials out at all. The vectors the stub returns are irrelevant here; only the *text* sent (the
    // graph-aware context header + content) is under test.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn caller() { target(); }\n");
    common::write(root, "src/b.rs", "fn target() {}\n");

    let server = common::EmbedStubServer::start(3, 8);
    let options = WalkOptions::builder()
        .build_graph(true)
        .embed(EmbedConfig::builder().base_url(server.base_url()).build())
        .build();

    let out1 = walk_checkout(root, &options).unwrap();
    let out2 = walk_checkout(root, &options).unwrap();

    let inputs1: Vec<Option<&str>> = out1
        .chunks
        .iter()
        .map(|c| c.embed_input.as_deref())
        .collect();
    let inputs2: Vec<Option<&str>> = out2
        .chunks
        .iter()
        .map(|c| c.embed_input.as_deref())
        .collect();
    assert!(
        inputs1.iter().all(Option::is_some),
        "every chunk must carry an embed_input once embed is configured"
    );
    assert_eq!(
        inputs1, inputs2,
        "embed_input must be byte-identical across repeat walks of the same checkout"
    );
}

#[test]
fn determinism_holds_across_all_committed_language_goldens() {
    // Walk every committed golden fixture twice and assert byte-identical output — broader coverage
    // than the single synthetic checkout above, over real multi-language fixtures.
    for fixture in [
        "sample-repo",
        "python-repo",
        "typescript-repo",
        "javascript-repo",
        "tsx-repo",
        "java-repo",
        "polyglot-repo",
    ] {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture);
        let options = WalkOptions::builder().build_graph(true).build();
        let g1 = walk_checkout(&root, &options).unwrap().graph;
        let g2 = walk_checkout(&root, &options).unwrap().graph;
        assert_eq!(g1, g2, "{fixture} must walk deterministically");
    }
}

// ── Raw-input order-independence ────────────────────────────────────────────────────────────────
// The three tests above prove `walk_checkout` doesn't depend on filesystem iteration order — but
// that's an indirect proof, since a test can't fully control what order a real directory walk
// visits entries in. With raw inputs the caller supplies the order directly, so these are more
// direct versions of the same guarantee: `graph::resolve`'s local-first policy and its sort+dedup
// must not depend on which `RawInput` reached `Indexer::push` first.

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Read `root/rel` into a `RawInput` carrying the same `/`-separated logical path `FsSource` itself
/// would produce. Duplicated from `tests/raw_inputs.rs`'s own `raw_input_for` rather than shared,
/// since integration-test binaries can't reach into each other's modules — only into `tests/common`.
fn read_raw_input(root: &Path, rel: &str) -> RawInput<'static> {
    let bytes = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    RawInput::new(rel.to_string(), bytes)
}

#[test]
fn raw_inputs_pushed_in_a_different_order_produce_an_identical_graph() {
    let root = fixture_root("polyglot-repo");
    let forward = [
        "java/App.java",
        "python/main.py",
        "python/models.py",
        "python/util.py",
        "rust/lib.rs",
        "ts/main.ts",
        "ts/service.ts",
    ];
    // A genuine shuffle, not a plain reversal (that's covered separately below) — interleaves
    // languages instead of visiting each one's files contiguously.
    let shuffled = [
        "rust/lib.rs",
        "ts/service.ts",
        "python/util.py",
        "java/App.java",
        "ts/main.ts",
        "python/models.py",
        "python/main.py",
    ];

    let options = IndexOptions::builder().build_graph(true).build();
    let g1 = index_inputs(
        forward.iter().map(|rel| read_raw_input(&root, rel)),
        &options,
    )
    .graph;
    let g2 = index_inputs(
        shuffled.iter().map(|rel| read_raw_input(&root, rel)),
        &options,
    )
    .graph;

    assert_eq!(
        g1.nodes, g2.nodes,
        "node identity/order must not depend on push order"
    );
    assert_eq!(
        g1.edges, g2.edges,
        "edge identity/order must not depend on push order"
    );
}

#[test]
fn running_index_inputs_twice_over_the_same_inputs_is_byte_identical() {
    let root = fixture_root("polyglot-repo");
    let files = [
        "java/App.java",
        "python/main.py",
        "python/models.py",
        "python/util.py",
        "rust/lib.rs",
        "ts/main.ts",
        "ts/service.ts",
    ];
    let options = IndexOptions::builder().build_graph(true).build();
    let build = || index_inputs(files.iter().map(|rel| read_raw_input(&root, rel)), &options);

    let out1 = build();
    let out2 = build();

    let json1 = serde_json::to_string_pretty(&out1.graph).unwrap();
    let json2 = serde_json::to_string_pretty(&out2.graph).unwrap();
    assert_eq!(json1, json2, "serialised graph must be byte-identical");
    assert_eq!(
        out1.chunks, out2.chunks,
        "chunks must be identically ordered across repeat runs over the same inputs"
    );
}

#[test]
fn a_reversed_input_order_still_resolves_the_same_cross_file_calls_edges() {
    let root = fixture_root("polyglot-repo");
    let forward = [
        "java/App.java",
        "python/main.py",
        "python/models.py",
        "python/util.py",
        "rust/lib.rs",
        "ts/main.ts",
        "ts/service.ts",
    ];

    let options = IndexOptions::builder().build_graph(true).build();
    let forward_out = index_inputs(
        forward.iter().map(|rel| read_raw_input(&root, rel)),
        &options,
    );
    let reversed_out = index_inputs(
        forward.iter().rev().map(|rel| read_raw_input(&root, rel)),
        &options,
    );

    let calls = |g: &lci_codegraph::Graph| -> Vec<lci_codegraph::GraphEdge> {
        g.edges
            .iter()
            .filter(|e| e.relation == "calls")
            .cloned()
            .collect()
    };
    assert_eq!(
        calls(&forward_out.graph),
        calls(&reversed_out.graph),
        "the exact set of resolved calls edges must not depend on input order"
    );
    // The graphs as a whole must also match — `resolve`'s sort+dedup canonicalises everything, not
    // just the calls edges singled out above.
    assert_eq!(forward_out.graph, reversed_out.graph);
}
