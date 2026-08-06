//! Per-language golden-graph harness, sibling to `tests/parity.rs` (which owns the original
//! Rust-only `sample-repo` golden and must not be touched here). Each fixture below is a small,
//! hand-readable checkout for one language (or, for `polyglot-repo`, several at once) that
//! exercises cross-file call resolution, def nesting (module/class → method), and at least one
//! genuinely ambiguous same-name call.
//!
//! Regenerate a golden intentionally with `UPDATE_GOLDEN=1 cargo test --test language_goldens`.

use std::path::{Path, PathBuf};

use lci_codegraph::{Graph, WalkOptions, walk_checkout};

fn fixture_root(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.graph.json"))
}

fn canonical_json(graph: &Graph) -> String {
    serde_json::to_string_pretty(graph).expect("graph serialises")
}

/// Walk `fixture`, and either (`UPDATE_GOLDEN=1`) write the golden, or assert the walk matches the
/// committed one exactly — the same contract as `tests/parity.rs`.
fn assert_matches_golden(fixture: &str) -> Graph {
    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(&fixture_root(fixture), &options).expect("walk fixture");
    let actual = canonical_json(&out.graph);

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        let path = golden_path(fixture);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("{actual}\n")).unwrap();
        eprintln!("golden updated at {}", path.display());
        return out.graph;
    }

    let path = golden_path(fixture);
    let expected = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read golden {} ({e}); regenerate with UPDATE_GOLDEN=1 cargo test --test language_goldens",
            path.display()
        )
    });
    assert_eq!(
        actual.trim(),
        expected.trim(),
        "structural graph for {fixture} drifted from the committed golden; if intended, \
         regenerate with UPDATE_GOLDEN=1 cargo test --test language_goldens"
    );
    out.graph
}

/// True iff a `calls` edge exists whose source and target live in different files — the high-value
/// case a golden must exercise (ADR-0086 R1/R5), not just intra-file resolution.
fn has_cross_file_call(g: &Graph) -> bool {
    g.edges.iter().any(|e| {
        if e.relation != "calls" {
            return false;
        }
        let src_file = g
            .nodes
            .iter()
            .find(|n| n.node_id == e.source)
            .map(|n| &n.source_file);
        let dst_file = g
            .nodes
            .iter()
            .find(|n| n.node_id == e.target)
            .map(|n| &n.source_file);
        matches!((src_file, dst_file), (Some(a), Some(b)) if a != b)
    })
}

/// True iff at least one `method` edge exists (a type container → a callable it defines) — the
/// nesting shape (module/class → method) every per-language fixture must exercise.
fn has_method_nesting(g: &Graph) -> bool {
    g.edges.iter().any(|e| e.relation == "method")
}

// ── Python ──────────────────────────────────────────────────────────────────────────────────────

#[test]
fn python_repo_graph_matches_committed_golden() {
    let g = assert_matches_golden("python-repo");
    assert!(has_cross_file_call(&g), "edges = {:?}", g.edges);
    assert!(has_method_nesting(&g), "edges = {:?}", g.edges);
}

#[test]
fn python_repo_run_bare_ambiguous_build_is_dropped() {
    let g = assert_matches_golden("python-repo");
    let run_bare = g
        .nodes
        .iter()
        .find(|n| n.label == "run_bare()")
        .expect("run_bare node");
    assert!(
        !g.edges
            .iter()
            .any(|e| e.relation == "calls" && e.source == run_bare.node_id),
        "bare ambiguous build() must be dropped, not fanned out; edges = {:?}",
        g.edges
    );
}

// ── TypeScript ──────────────────────────────────────────────────────────────────────────────────

#[test]
fn typescript_repo_graph_matches_committed_golden() {
    let g = assert_matches_golden("typescript-repo");
    assert!(has_cross_file_call(&g), "edges = {:?}", g.edges);
    assert!(has_method_nesting(&g), "edges = {:?}", g.edges);
}

#[test]
fn typescript_repo_run_bare_ambiguous_build_is_dropped() {
    let g = assert_matches_golden("typescript-repo");
    let run_bare = g
        .nodes
        .iter()
        .find(|n| n.label == "runBare()")
        .expect("runBare node");
    assert!(
        !g.edges
            .iter()
            .any(|e| e.relation == "calls" && e.source == run_bare.node_id),
        "bare ambiguous build() must be dropped, not fanned out; edges = {:?}",
        g.edges
    );
}

// ── JavaScript (incl. JSX) ──────────────────────────────────────────────────────────────────────

#[test]
fn javascript_repo_graph_matches_committed_golden() {
    let g = assert_matches_golden("javascript-repo");
    assert!(has_cross_file_call(&g), "edges = {:?}", g.edges);
    assert!(has_method_nesting(&g), "edges = {:?}", g.edges);
    // The .jsx file's arrow component must be extracted and called, proving JSX bodies don't wreck
    // the parse for plain JavaScript.
    assert!(
        g.nodes.iter().any(|n| n.label == "Button()"),
        "jsx arrow component node; nodes = {:?}",
        g.nodes
    );
}

#[test]
fn javascript_repo_run_bare_ambiguous_build_is_dropped() {
    let g = assert_matches_golden("javascript-repo");
    let run_bare = g
        .nodes
        .iter()
        .find(|n| n.label == "runBare()")
        .expect("runBare node");
    assert!(
        !g.edges
            .iter()
            .any(|e| e.relation == "calls" && e.source == run_bare.node_id),
        "bare ambiguous build() must be dropped, not fanned out; edges = {:?}",
        g.edges
    );
}

// ── TSX/JSX (React-shaped components) ──────────────────────────────────────────────────────────

#[test]
fn tsx_repo_graph_matches_committed_golden() {
    let g = assert_matches_golden("tsx-repo");
    assert!(has_cross_file_call(&g), "edges = {:?}", g.edges);
    assert!(has_method_nesting(&g), "edges = {:?}", g.edges);
    // The JSX-returning arrow component must itself be a callable node reached through the TSX
    // (not plain TS) grammar.
    assert!(
        g.nodes.iter().any(|n| n.label == "App()"),
        "App component node; nodes = {:?}",
        g.nodes
    );
}

#[test]
fn tsx_repo_run_bare_ambiguous_build_is_dropped() {
    let g = assert_matches_golden("tsx-repo");
    let run_bare = g
        .nodes
        .iter()
        .find(|n| n.label == "runBare()")
        .expect("runBare node");
    assert!(
        !g.edges
            .iter()
            .any(|e| e.relation == "calls" && e.source == run_bare.node_id),
        "bare ambiguous build() must be dropped, not fanned out; edges = {:?}",
        g.edges
    );
}

// ── Java ────────────────────────────────────────────────────────────────────────────────────────

#[test]
fn java_repo_graph_matches_committed_golden() {
    let g = assert_matches_golden("java-repo");
    assert!(has_cross_file_call(&g), "edges = {:?}", g.edges);
    assert!(has_method_nesting(&g), "edges = {:?}", g.edges);
}

#[test]
fn java_repo_run_bare_ambiguous_build_is_dropped() {
    let g = assert_matches_golden("java-repo");
    let run_bare = g
        .nodes
        .iter()
        .find(|n| n.label == "runBare()")
        .expect("runBare node");
    assert!(
        !g.edges
            .iter()
            .any(|e| e.relation == "calls" && e.source == run_bare.node_id),
        "bare ambiguous build() must be dropped, not fanned out; edges = {:?}",
        g.edges
    );
}

// ── Polyglot (Python + TypeScript + Rust + Java in one checkout) ──────────────────────────────────

#[test]
fn polyglot_repo_graph_matches_committed_golden() {
    let g = assert_matches_golden("polyglot-repo");
    assert!(has_cross_file_call(&g), "edges = {:?}", g.edges);
    assert!(has_method_nesting(&g), "edges = {:?}", g.edges);
    // Every language in the checkout produced at least one node — the walk genuinely covers all of
    // them in one pass, not just the first language it happens to see.
    for dir in ["python/", "ts/", "rust/", "java/"] {
        assert!(
            g.nodes.iter().any(|n| n.source_file.starts_with(dir)),
            "expected at least one node under {dir}; nodes = {:?}",
            g.nodes
        );
    }
}

#[test]
fn polyglot_repo_python_run_bare_ambiguous_build_is_dropped() {
    let g = assert_matches_golden("polyglot-repo");
    let run_bare = g
        .nodes
        .iter()
        .find(|n| n.label == "run_bare()" && n.source_file.starts_with("python/"))
        .expect("python run_bare node");
    assert!(
        !g.edges
            .iter()
            .any(|e| e.relation == "calls" && e.source == run_bare.node_id),
        "bare ambiguous build() must be dropped, not fanned out; edges = {:?}",
        g.edges
    );
}

/// Sanity check independent of any golden: every fixture directory under `tests/fixtures/` this
/// suite exercises must exist and contain no `.git` (fixtures are plain checkouts, not real repos).
#[test]
fn fixtures_have_no_embedded_git_dir() {
    for name in [
        "python-repo",
        "typescript-repo",
        "javascript-repo",
        "tsx-repo",
        "java-repo",
        "polyglot-repo",
    ] {
        let root = fixture_root(name);
        assert!(root.is_dir(), "{name} fixture must exist at {root:?}");
        assert!(
            !Path::new(&root).join(".git").exists(),
            "{name} fixture must not contain a .git directory"
        );
    }
}
