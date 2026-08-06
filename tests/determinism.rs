//! Determinism guarantees `walk_checkout` must uphold (ADR-0086): the structural graph is
//! canonicalised (sorted + deduped, see `graph::resolve`) *specifically* so it's stable to snapshot
//! as a golden and stable to submit — this suite is the black-box proof of that property through the
//! public API, not just the `graph` module's own internal unit test
//! (`graph::tests::output_is_deterministic`).

mod common;

use lci_codegraph::{WalkOptions, walk_checkout};

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
