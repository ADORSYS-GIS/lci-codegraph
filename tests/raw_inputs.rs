//! Proves the raw-inputs cutover (ADR-0086): `crate::input`'s content-level core produces the exact
//! same output whether it is fed by `walk::FsSource` (a filesystem checkout) or by hand-built
//! `RawInput`s carrying the same paths and bytes — and that both can be mixed in a single run. If a
//! raw-input run of the same paths/bytes a filesystem walk would produce ever yielded a *different*
//! graph or chunk set, the "the filesystem is just one reader among several" claim documented at the
//! top of `src/input.rs` would be false; that's the specific regression every test in this file
//! guards against.

mod common;

use std::path::{Path, PathBuf};

use lci_codegraph::{
    Chunk, FsSource, IndexOptions, Indexer, RawInput, WalkOptions, index_inputs, walk_checkout,
};

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// Read `root/rel` into a `RawInput` carrying the same `/`-separated logical path `FsSource` itself
/// would produce for that file — the point being that a raw-input caller and the filesystem reader
/// hand the indexer identical `RawInput`s, byte for byte.
fn raw_input_for(root: &Path, rel: &str) -> RawInput<'static> {
    let bytes = std::fs::read(root.join(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    RawInput::new(rel.to_string(), bytes)
}

/// Canonical order for comparing two chunk sets that may have been produced by readers with
/// different (but equally valid) iteration orders — `Indexer` never promises chunk order is
/// input-order-independent, only `graph::resolve`'s sort+dedup does that for the graph.
fn sorted_chunks(mut chunks: Vec<Chunk>) -> Vec<Chunk> {
    chunks.sort_by(|a, b| {
        (&a.file_path, a.start_line, a.end_line, &a.symbol_name).cmp(&(
            &b.file_path,
            b.start_line,
            b.end_line,
            &b.symbol_name,
        ))
    });
    chunks
}

#[test]
fn raw_inputs_produce_the_same_graph_as_the_filesystem_reader() {
    let root = fixture_root("sample-repo");
    let known_files = ["src/main.rs", "src/math.rs", "src/shapes.rs"];

    let raw_inputs: Vec<RawInput<'static>> = known_files
        .iter()
        .map(|rel| raw_input_for(&root, rel))
        .collect();

    let raw_out = index_inputs(
        raw_inputs,
        &IndexOptions::builder().build_graph(true).build(),
    );
    let walk_out = walk_checkout(&root, &WalkOptions::builder().build_graph(true).build())
        .expect("walk fixture");

    // `Graph` is sorted+deduped by `resolve` (see `src/graph/mod.rs`), so a plain `Vec` equality on
    // nodes/edges is meaningful regardless of the order either reader happened to hand inputs over in.
    assert_eq!(
        raw_out.graph.nodes, walk_out.graph.nodes,
        "raw-input nodes must match the filesystem reader's nodes"
    );
    assert_eq!(
        raw_out.graph.edges, walk_out.graph.edges,
        "raw-input edges must match the filesystem reader's edges"
    );
    assert_eq!(
        sorted_chunks(raw_out.chunks),
        sorted_chunks(walk_out.chunks),
        "raw-input chunks must match the filesystem reader's chunks"
    );
}

#[test]
fn raw_inputs_produce_the_same_graph_as_the_filesystem_reader_for_a_polyglot_repo() {
    let root = fixture_root("polyglot-repo");
    let known_files = [
        "java/App.java",
        "python/main.py",
        "python/models.py",
        "python/util.py",
        "rust/lib.rs",
        "ts/main.ts",
        "ts/service.ts",
    ];

    let raw_inputs: Vec<RawInput<'static>> = known_files
        .iter()
        .map(|rel| raw_input_for(&root, rel))
        .collect();

    let raw_out = index_inputs(
        raw_inputs,
        &IndexOptions::builder().build_graph(true).build(),
    );
    let walk_out = walk_checkout(&root, &WalkOptions::builder().build_graph(true).build())
        .expect("walk fixture");

    assert_eq!(raw_out.graph.nodes, walk_out.graph.nodes);
    assert_eq!(raw_out.graph.edges, walk_out.graph.edges);
    assert_eq!(
        sorted_chunks(raw_out.chunks),
        sorted_chunks(walk_out.chunks)
    );

    // At minimum: every language in the polyglot fixture contributed a `calls` edge from raw inputs
    // alone — this is the headline capability, not just a parity curiosity.
    for lang_dir in ["java/", "python/", "rust/", "ts/"] {
        assert!(
            raw_out.graph.edges.iter().any(|e| {
                e.relation == "calls"
                    && raw_out
                        .graph
                        .nodes
                        .iter()
                        .find(|n| n.node_id == e.source)
                        .is_some_and(|n| n.source_file.starts_with(lang_dir))
            }),
            "expected a calls edge sourced from {lang_dir}, edges = {:?}",
            raw_out.graph.edges
        );
    }
}

#[test]
fn fs_source_is_usable_directly_as_a_public_iterator() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn a() {}\n");
    common::write(root, "src/b.rs", "fn b() {}\n");
    // Operator-default junk dir: pruned by the ignore layer before `FsSource` ever yields a
    // `RawInput` for it, so it must show up in `paths_ignored()`, not in the collected paths.
    common::write(root, "target/generated.rs", "fn gen() {}\n");

    let options = WalkOptions::builder().build();
    let mut source = FsSource::new(root, &options).expect("construct FsSource");

    // `by_ref()` so `source` itself is still owned afterward — proving `FsSource` is a plain,
    // reusable `Iterator`, not something that must be consumed by value through a driver function.
    let mut paths: Vec<String> = source
        .by_ref()
        .map(|input| input.path.into_owned())
        .collect();
    paths.sort();

    assert_eq!(paths, vec!["src/a.rs".to_string(), "src/b.rs".to_string()]);
    assert!(
        source.paths_ignored() >= 1,
        "target/ must have been pruned and counted, even after the iterator is exhausted"
    );
}

#[test]
fn fs_source_new_on_a_nonexistent_root_does_not_error_and_yields_nothing() {
    // Pinned, not designed: neither `IgnoreList::build` nor `ignore::WalkBuilder` stats the root
    // eagerly, so `FsSource::new` succeeds even for a root that doesn't exist. The underlying walk
    // surfaces the missing root as an `Err` entry the first time `next()` is polled; `FsSource`'s
    // `Iterator` impl logs and skips any walk error via the same `continue` path it uses for any
    // other unreadable entry — so the caller sees a plain empty iterator, not a construction error
    // and not a panic.
    let dir = tempfile::tempdir().unwrap();
    let missing_root = dir.path().join("does-not-exist");

    let mut source = FsSource::new(&missing_root, &WalkOptions::builder().build())
        .expect("FsSource::new does not validate that root exists");
    let items: Vec<_> = source.by_ref().collect();

    assert!(items.is_empty());
    assert_eq!(
        source.paths_ignored(),
        0,
        "the missing root is surfaced as a walk error, not something the ignore layer pruned"
    );
}

#[test]
fn fs_source_over_an_empty_directory_yields_nothing_and_ignores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut source =
        FsSource::new(dir.path(), &WalkOptions::builder().build()).expect("construct FsSource");
    let items: Vec<_> = source.by_ref().collect();

    assert!(items.is_empty());
    assert_eq!(
        source.paths_ignored(),
        0,
        "nothing to prune in an empty tree"
    );
}

#[test]
fn a_caller_can_mix_an_fs_source_with_a_synthetic_in_memory_raw_input() {
    // This is the capability the cutover exists to enable: an `Indexer` doesn't care whether an input
    // came from a filesystem walk or was synthesised in memory — a caller can freely interleave both
    // into the SAME indexing run and get one coherent graph out, including edges between them.
    let root = fixture_root("sample-repo");
    let mut source =
        FsSource::new(&root, &WalkOptions::builder().build()).expect("construct FsSource");

    let mut indexer = Indexer::new(IndexOptions::builder().build_graph(true).build());
    for input in &mut source {
        indexer.push(input);
    }
    // A synthetic buffer that exists nowhere on disk, calling into `src/math.rs::add` from the
    // fixture it was just walked alongside.
    indexer.push(RawInput::text(
        "synthetic/buffer.rs",
        "fn synth() { add(1, 2); }\n",
    ));
    let out = indexer.finish();

    let synth_node = out
        .graph
        .nodes
        .iter()
        .find(|n| n.label == "synth()")
        .expect("the synthetic input's own function was extracted into the graph");

    assert!(
        out.graph.edges.iter().any(|e| {
            e.relation == "calls"
                && e.source == synth_node.node_id
                && out.graph.nodes.iter().any(|n| {
                    n.node_id == e.target && n.source_file == "src/math.rs" && n.label == "add()"
                })
        }),
        "expected a calls edge from the synthetic buffer into the fixture's src/math.rs::add, edges = {:?}",
        out.graph.edges
    );
}

// ── Golden parity from raw inputs alone ─────────────────────────────────────────────────────────
// `tests/language_goldens.rs` (and `tests/parity.rs`, for sample-repo) assert each fixture's graph
// against a committed golden by driving it through `walk_checkout`. This is the raw-input
// equivalent: read the SAME fixture off disk by hand, feed it to `index_inputs` instead of
// `walk_checkout`, and assert it matches the SAME committed golden — the strongest available
// statement that the filesystem reader and a raw-input caller are genuinely interchangeable, across
// every supported language in one sweep, not just the two hand-picked fixtures above.

/// Recursively read every regular file under `root` into a `RawInput` carrying the same
/// `/`-separated path relative to `root` that `FsSource` itself would produce. No ignore-list layer
/// here on purpose — pruning paths is the FS *reader*'s job (`crate::walk`), and none of the golden
/// fixtures contain anything that needs pruning (see `fixtures_have_no_embedded_git_dir` in
/// `tests/language_goldens.rs`).
fn read_fixture_as_raw_inputs(root: &Path) -> Vec<RawInput<'static>> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<RawInput<'static>>) {
        let entries =
            std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
        for entry in entries {
            let entry = entry.expect("dir entry");
            let file_type = entry.file_type().expect("file type");
            let path = entry.path();
            if file_type.is_dir() {
                walk(&path, root, out);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .expect("entry is under root")
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
            out.push(RawInput::new(rel, bytes));
        }
    }
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out
}

#[test]
fn raw_inputs_match_the_committed_golden_for_every_language_fixture() {
    // Every fixture directory under `tests/golden/` that a per-language golden exists for —
    // `sample-repo` is the original Rust-only fixture owned by `tests/parity.rs`, the rest are
    // `tests/language_goldens.rs`'s. Do NOT regenerate a golden here if one mismatches: that's a
    // real parity break between the two readers, not something this test is allowed to paper over.
    for fixture in [
        "sample-repo",
        "python-repo",
        "typescript-repo",
        "javascript-repo",
        "tsx-repo",
        "java-repo",
        "typescript-interface-repo",
        "java-interface-repo",
        "polyglot-repo",
    ] {
        let root = fixture_root(fixture);
        let inputs = read_fixture_as_raw_inputs(&root);
        assert!(
            !inputs.is_empty(),
            "{fixture}: directory walk found no files at {root:?}"
        );

        let out = index_inputs(inputs, &IndexOptions::builder().build_graph(true).build());
        let actual = serde_json::to_string_pretty(&out.graph).expect("graph serialises");

        let golden_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/golden")
            .join(format!("{fixture}.graph.json"));
        let expected = std::fs::read_to_string(&golden_path).unwrap_or_else(|e| {
            panic!(
                "read golden {} ({e}); this test must never regenerate it",
                golden_path.display()
            )
        });

        assert_eq!(
            actual.trim(),
            expected.trim(),
            "{fixture}: raw-input graph drifted from the SAME golden walk_checkout produces — the \
             two readers must be interchangeable"
        );
    }
}
