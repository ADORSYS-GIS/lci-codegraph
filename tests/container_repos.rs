//! Runs the extractor over real-world, unmodified open-source checkouts — input nobody wrote for
//! these tests — and asserts the invariants that must hold no matter what source code it sees. This
//! is the suite most likely to catch a real resolver bug: `tests/parity.rs`'s golden fixture is
//! small and hand-written, so it can't exercise the long tail of syntax real repos contain.
//!
//! Repos are pinned by **exact commit SHA** (never a branch/tag ref) so the suite is reproducible —
//! a moving target would make a red run undebuggable a month later. One small, well-known repo per
//! language this crate has a real extractor for (Rust/Python/TypeScript/Java, per
//! `src/graph/mod.rs`'s module doc):
//!
//! | language   | repo                              | pinned commit                              |
//! |------------|------------------------------------|---------------------------------------------|
//! | Rust       | `rust-lang/log`                    | `037d7a58f6ad184abb3afc4db81d37c43a5696ec` |
//! | Python     | `pallets/itsdangerous`             | `672971d66a2ef9f85151e53283113f33d642dabd` |
//! | TypeScript | `sindresorhus/is`                  | `7821031c66cdeb7256a0feb2d506535f9e84fcaf` |
//! | Java       | `stleary/JSON-java`                | `22ab2fb7245bb0c1a766c6aa11c905aa82315ef3` |
//!
//! All four are small (each well under 3MB checked out) but real, actively-used libraries with
//! multiple source files, so cross-file call resolution has a genuine chance to fire.
//!
//! For each repo, run the extractor **twice** inside one container (build once, run twice) and
//! assert:
//! - clone + build + both runs complete without panicking, exit status 0;
//! - a non-empty node set and a non-empty edge set;
//! - at least one `calls` edge somewhere across the four repos (cross-file resolution fired);
//! - no dangling edges — every `source`/`target` references a `node_id` that exists;
//! - `node_id`s are unique and every `start_line >= 1`;
//! - the two runs over the same checkout produce byte-identical JSON (determinism).
//!
//! Skips cleanly (prints and returns, does not fail) when no Docker daemon is reachable.
//!
//! **Slow.** One container does: pull `rust:slim-bookworm`, `apt-get install git`, build the
//! `dump-graph` example once, `git clone` four real repos over the network, then walk each twice.
//! Budget several minutes, more on a cold Cargo-registry/image cache. A named Docker volume caches
//! the Cargo registry and target dir across repeat local runs
//! (`lci-codegraph-container-repos-{cargo-registry,target}`); the repo clones themselves are not
//! cached (they're the point of the test — a fresh, real checkout every time).

#[path = "support/mod.rs"]
mod support;

use std::process::Command;
use std::time::{Duration, Instant};

/// Generous: cold image pull + `apt-get install git` + full crate build (incl. tree-sitter grammars)
/// + four real network clones + two walks each.
const TIMEOUT: Duration = Duration::from_secs(900);

const CARGO_REGISTRY_VOLUME: &str = "lci-codegraph-container-repos-cargo-registry";
const TARGET_VOLUME: &str = "lci-codegraph-container-repos-target";

/// (short name, GitHub `owner/repo`, pinned commit SHA). SHA verified reachable via
/// `git ls-remote` against the live repo when this suite was written; pinned so a force-push or
/// rewritten history upstream cannot silently change what this suite measures (it would instead
/// fail the clone outright, which is the correct failure mode).
const REPOS: &[(&str, &str, &str)] = &[
    ("log", "rust-lang/log", "037d7a58f6ad184abb3afc4db81d37c43a5696ec"),
    (
        "itsdangerous",
        "pallets/itsdangerous",
        "672971d66a2ef9f85151e53283113f33d642dabd",
    ),
    (
        "is",
        "sindresorhus/is",
        "7821031c66cdeb7256a0feb2d506535f9e84fcaf",
    ),
    (
        "json-java",
        "stleary/JSON-java",
        "22ab2fb7245bb0c1a766c6aa11c905aa82315ef3",
    ),
];

struct RepoRun {
    name: &'static str,
    first: support::GraphJson,
    /// Raw JSON text of both runs, kept around only to assert byte-identical determinism (parsing
    /// into `GraphJson` and re-comparing structurally would mask a determinism bug in field
    /// ordering/formatting that a byte comparison catches).
    first_raw: String,
    second_raw: String,
}

/// One container: builds `dump-graph` once, then for every pinned repo clones it, walks it twice,
/// and writes `<name>.first.json` / `<name>.second.json` (+ a combined `<name>.stats.txt`) to the
/// host-mounted output dir. Returns once the whole script exits; per-repo parsing/assertions happen
/// back on the host.
fn clone_build_and_walk_all_repos(test_name: &str) -> (Vec<RepoRun>, Duration) {
    let crate_root = support::crate_root();
    let output_dir = tempfile::tempdir().expect("tempdir for container output");

    let mut clone_and_walk = String::new();
    for (name, repo, sha) in REPOS {
        // Fetch the exact pinned SHA directly (GitHub allows fetching by commit hash, not just
        // refs) rather than `git clone` + `checkout <sha>` off the default branch — the SHAs above
        // are pinned to what was HEAD when this suite was written, and upstream will keep moving,
        // so a full clone would still work today but stops being the fast path over time. A shallow
        // fetch of exactly that commit is both faster and correct regardless of where the default
        // branch has since moved.
        clone_and_walk.push_str(&format!(
            "mkdir -p /repos/{name} && \
             git -C /repos/{name} init --quiet && \
             git -C /repos/{name} remote add origin https://github.com/{repo}.git && \
             git -C /repos/{name} fetch --quiet --depth 1 origin {sha} && \
             git -C /repos/{name} checkout --quiet FETCH_HEAD && \
             /cargo-target/release/examples/dump-graph /repos/{name} \
                 >/output/{name}.first.json 2>>/output/{name}.stats.txt && \
             /cargo-target/release/examples/dump-graph /repos/{name} \
                 >/output/{name}.second.json 2>>/output/{name}.stats.txt\n"
        ));
    }

    let script = format!(
        "set -e\n\
         apt-get update -qq >/dev/null && \
             apt-get install -y -qq --no-install-recommends git gcc >/dev/null\n\
         mkdir -p /build /repos\n\
         cp -r /workspace-ro/. /build/\n\
         cd /build\n\
         cargo build --example dump-graph --release\n\
         {clone_and_walk}"
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
        &format!("{TARGET_VOLUME}:/cargo-target"),
        "-v",
        &format!("{}:/output", output_dir.path().display()),
        "-e",
        "CARGO_TARGET_DIR=/cargo-target",
        "rust:slim-bookworm",
        "sh",
        "-c",
        &script,
    ]);

    let start = Instant::now();
    let output = support::run_with_timeout(cmd, TIMEOUT, test_name);
    let elapsed = start.elapsed();

    assert!(
        output.status.success(),
        "{test_name}: docker run failed: status={:?}\n--- docker stdout ---\n{}\n\
         --- docker stderr ---\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let mut runs = Vec::with_capacity(REPOS.len());
    for (name, _repo, _sha) in REPOS {
        let stats_path = output_dir.path().join(format!("{name}.stats.txt"));
        let stats = std::fs::read_to_string(&stats_path).unwrap_or_default();

        let first_raw = std::fs::read_to_string(output_dir.path().join(format!("{name}.first.json")))
            .unwrap_or_else(|e| panic!("{test_name}: read {name}.first.json: {e}\nstats: {stats}"));
        let second_raw =
            std::fs::read_to_string(output_dir.path().join(format!("{name}.second.json")))
                .unwrap_or_else(|e| {
                    panic!("{test_name}: read {name}.second.json: {e}\nstats: {stats}")
                });
        let first: support::GraphJson = serde_json::from_str(&first_raw).unwrap_or_else(|e| {
            panic!("{test_name}: {name} first run JSON did not parse: {e}\nraw: {first_raw}")
        });

        runs.push(RepoRun {
            name,
            first,
            first_raw,
            second_raw,
        });
    }

    (runs, elapsed)
}

#[test]
fn extractor_holds_its_invariants_over_real_world_repos() {
    let test_name = "extractor_holds_its_invariants_over_real_world_repos";
    if !support::docker_available() {
        support::print_skip_message(test_name);
        return;
    }

    let (runs, elapsed) = clone_build_and_walk_all_repos(test_name);
    eprintln!("{test_name}: total wall-clock {elapsed:?} for {} repos", runs.len());

    let mut any_calls_edge = false;

    for run in &runs {
        let RepoRun {
            name,
            first,
            first_raw,
            second_raw,
        } = run;

        assert!(!first.nodes.is_empty(), "{name}: no nodes emitted");
        assert!(!first.edges.is_empty(), "{name}: no edges emitted");

        let dangling = first.dangling_edges();
        assert!(
            dangling.is_empty(),
            "{name}: dangling edges reference a node_id nothing emitted: {dangling:?}"
        );

        let dupes = first.duplicate_node_ids();
        assert!(dupes.is_empty(), "{name}: duplicate node_ids: {dupes:?}");

        for node in &first.nodes {
            assert!(
                node.start_line >= 1,
                "{name}: node {} has start_line {} (< 1)",
                node.node_id,
                node.start_line
            );
        }

        // Determinism: two runs over the identical checkout must produce byte-identical JSON. A
        // structural (parsed) comparison would hide a determinism bug in ordering/formatting, so
        // this compares the raw bytes as `dump-graph` actually emitted them.
        assert_eq!(
            first_raw, second_raw,
            "{name}: two walks of the same checkout produced different JSON (non-deterministic \
             output — likely an unstable sort or a HashMap iteration order leaking through)"
        );

        any_calls_edge |= first.edges.iter().any(|e| e.relation == "calls");

        eprintln!(
            "{name}: {} nodes, {} edges ({} calls)",
            first.nodes.len(),
            first.edges.len(),
            first.edges.iter().filter(|e| e.relation == "calls").count()
        );
    }

    assert!(
        any_calls_edge,
        "expected at least one `calls` edge across the four real repos — cross-file resolution \
         should have fired somewhere; got none in any of {:?}",
        runs.iter().map(|r| r.name).collect::<Vec<_>>()
    );
}
