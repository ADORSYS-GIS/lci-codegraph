//! Proves the crate actually compiles and runs on the intended deployment target — Linux, off the
//! host (macOS/whatever-dev-box) toolchain — for both glibc and musl. The tree-sitter grammars
//! compile C via a `cc` build-script (see `Cargo.toml`'s tree-sitter-* dependencies), and musl in
//! particular needs its own libc headers/linker; this is the only suite that would catch a grammar
//! that silently fails to cross-compile.
//!
//! Builds `examples/dump-graph.rs` (a tiny CLI: walk a path with the graph on, print `Graph` as JSON
//! to stdout) inside `rust:slim-bookworm` (glibc) and `rust:alpine` (musl, needs `musl-dev` +
//! `build-base` installed for the C grammars), runs it against the committed fixture repo, and
//! asserts the JSON parses and contains the symbols the fixture is known to produce (cross-checked
//! against `tests/golden/sample-repo.graph.json`).
//!
//! Skips cleanly (prints and returns, does not fail) when no Docker daemon is reachable.
//!
//! **Slow.** Each variant: cold, this is an image pull (rust:slim-bookworm ~800MB, rust:alpine
//! ~200MB) plus a full `cargo build` of every dependency including compiling each tree-sitter
//! grammar's C sources — multiple minutes. A named Docker volume caches the Cargo registry AND the
//! target dir across repeat runs (`lci-codegraph-container-build-{cargo-registry,target-<variant>}`)
//! so a second local run is much faster than the first; CI has no such warm cache.

#[path = "support/mod.rs"]
mod support;

use std::process::Command;
use std::time::{Duration, Instant};

/// Generous: a cold run pulls a multi-hundred-MB base image and compiles the full dependency graph
/// (incl. every tree-sitter C grammar) from scratch into a fresh volume.
const BUILD_TIMEOUT: Duration = Duration::from_secs(900);

const CARGO_REGISTRY_VOLUME: &str = "lci-codegraph-container-build-cargo-registry";

struct BuildOutcome {
    graph: support::GraphJson,
    elapsed: Duration,
}

/// Run one variant: mount the crate source read-only, copy it to a writable working dir inside the
/// container (keeps the *mount* read-only per the task's constraint while still letting `cargo`
/// write `Cargo.lock`/build artifacts), install whatever the variant needs for the `cc` build
/// scripts, build the example in release mode, and run it against the fixture repo. The graph JSON
/// is written to a host-mounted output dir (not captured off stdout) so container package-manager
/// noise on stdout/stderr never has to be filtered out.
fn build_and_run_variant(
    test_name: &str,
    image: &str,
    variant: &str,
    setup_script: &str,
) -> BuildOutcome {
    let crate_root = support::crate_root();
    let output_dir = tempfile::tempdir().expect("tempdir for container output");
    let target_volume = format!("lci-codegraph-container-build-target-{variant}");

    let script = format!(
        "set -e\n\
         {setup_script}\n\
         mkdir -p /build\n\
         cp -r /workspace-ro/. /build/\n\
         cd /build\n\
         cargo build --example dump-graph --release\n\
         \"$CARGO_TARGET_DIR/release/examples/dump-graph\" /build/tests/fixtures/sample-repo \
             >/output/graph.json 2>/output/stats.txt\n"
    );

    let mut cmd = Command::new("docker");
    cmd.args([
        "run",
        "--rm",
        "-v",
        &format!("{}:/workspace-ro:ro", crate_root.display()),
        "-v",
        &format!("{CARGO_REGISTRY_VOLUME}:/usr/local/cargo/registry"),
        "-v",
        &format!("{target_volume}:/cargo-target"),
        "-v",
        &format!("{}:/output", output_dir.path().display()),
        "-e",
        "CARGO_TARGET_DIR=/cargo-target",
        image,
        "sh",
        "-c",
        &script,
    ]);

    let start = Instant::now();
    let output = support::run_with_timeout(cmd, BUILD_TIMEOUT, test_name);
    let elapsed = start.elapsed();

    let stats = std::fs::read_to_string(output_dir.path().join("stats.txt")).unwrap_or_default();
    assert!(
        output.status.success(),
        "{test_name}: `docker run` for {image} failed: status={:?}\n--- docker stdout ---\n{}\n\
         --- docker stderr ---\n{}\n--- in-container stats.txt (if any) ---\n{stats}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let graph_path = output_dir.path().join("graph.json");
    let graph_json = std::fs::read_to_string(&graph_path).unwrap_or_else(|e| {
        panic!(
            "{test_name}: read {} failed: {e}; stats.txt: {stats}",
            graph_path.display()
        )
    });
    let graph: support::GraphJson = serde_json::from_str(&graph_json).unwrap_or_else(|e| {
        panic!("{test_name}: dump-graph output did not parse as JSON: {e}\nraw: {graph_json}")
    });

    BuildOutcome { graph, elapsed }
}

/// The fixture's known shape (cross-checked against `tests/golden/sample-repo.graph.json`): `add()`
/// is defined in `src/math.rs` at line 2, and `main()` calls it. If a variant's build silently
/// produced a broken/empty parser (e.g. a grammar that failed to link and got skipped), this is the
/// assertion that would catch it.
fn assert_fixture_symbols_present(test_name: &str, graph: &support::GraphJson) {
    assert!(!graph.nodes.is_empty(), "{test_name}: no nodes emitted");
    assert!(!graph.edges.is_empty(), "{test_name}: no edges emitted");
    assert!(
        graph.nodes.iter().any(|n| n.node_id == "src/math.rs#2:add"
            && n.label == "add()"
            && n.source_file == "src/math.rs"
            && n.start_line == 2),
        "{test_name}: missing the fixture's add() node; nodes = {:?}",
        graph.nodes
    );
    assert!(
        graph.edges.iter().any(|e| e.relation == "calls"
            && e.source == "src/main.rs#3:main"
            && e.target == "src/math.rs#2:add"),
        "{test_name}: missing main() -> add() calls edge; edges = {:?}",
        graph.edges
    );
    assert!(
        graph.dangling_edges().is_empty(),
        "{test_name}: dangling edges = {:?}",
        graph.dangling_edges()
    );
}

#[test]
fn crate_builds_and_extracts_the_fixture_graph_on_glibc() {
    let test_name = "crate_builds_and_extracts_the_fixture_graph_on_glibc";
    if !support::docker_available() {
        support::print_skip_message(test_name);
        return;
    }
    let setup = "apt-get update -qq >/dev/null && \
                 apt-get install -y -qq --no-install-recommends gcc >/dev/null";
    let outcome = build_and_run_variant(test_name, "rust:slim-bookworm", "glibc", setup);
    assert_fixture_symbols_present(test_name, &outcome.graph);
    eprintln!("{test_name}: {:?}", outcome.elapsed);
}

#[test]
fn crate_builds_and_extracts_the_fixture_graph_on_musl() {
    let test_name = "crate_builds_and_extracts_the_fixture_graph_on_musl";
    if !support::docker_available() {
        support::print_skip_message(test_name);
        return;
    }
    // `build-base` pulls gcc/make/musl-dev transitively on Alpine, but pin `musl-dev` explicitly too
    // since it's the one the tree-sitter `cc` build scripts actually need headers from.
    let setup = "apk add --no-cache build-base musl-dev >/dev/null";
    let outcome = build_and_run_variant(test_name, "rust:alpine", "musl", setup);
    assert_fixture_symbols_present(test_name, &outcome.graph);
    eprintln!("{test_name}: {:?}", outcome.elapsed);
}
