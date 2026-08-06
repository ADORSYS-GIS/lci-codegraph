//! Shared helpers for the Docker-backed test suites (`container_neo4j`, `container_build`,
//! `container_repos`). Not a test binary itself — pulled in via `#[path = "support/mod.rs"] mod
//! support;` in each suite (the standard way to share code across separate `tests/*.rs` binaries
//! without cargo treating this file as its own integration-test target).
//!
//! `dead_code` is allowed at module level: each `tests/*.rs` compiles this file into its own
//! separate crate, and no single suite uses every helper here (e.g. only `container_neo4j` needs
//! [`fixture_root`]) — that's expected of a shared toolkit, not a real dead-code smell.
#![allow(dead_code)]

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use serde::Deserialize;

/// Absolute path to this crate's checkout root (`CARGO_MANIFEST_DIR`), used to mount the source
/// into containers and to resolve the fixture repo.
pub fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The committed, read-only fixture repo other suites (`tests/parity.rs`) already exercise. We only
/// ever read it.
pub fn fixture_root() -> PathBuf {
    crate_root().join("tests/fixtures/sample-repo")
}

/// True when a Docker daemon answers `docker info` within a few seconds. Every container test must
/// call this (or [`skip_message`]) first and skip cleanly — not fail — when it's false, so this
/// suite runs green in any environment without silently no-op'ing on the box that actually has
/// Docker (CI-with-Docker, a dev laptop with Docker Desktop running).
pub fn docker_available() -> bool {
    Command::new("docker")
        .args(["info"])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

/// Print the standard "skipping, no Docker" message. Call this immediately before `return`ing early
/// from a container test when [`docker_available`] is false.
pub fn print_skip_message(test_name: &str) {
    eprintln!(
        "SKIP {test_name}: Docker daemon not reachable (`docker info` failed). \
         This suite requires a local Docker daemon; start Docker and re-run with \
         `cargo test --features container-tests`."
    );
}

/// Run `cmd` to completion, killing it if it outlives `timeout`. These suites shell out to `docker
/// run` for real builds/clones/compiles — slow by nature — so every invocation needs an explicit,
/// generous bound rather than trusting the process to exit (a hung `docker run` would otherwise wedge
/// the whole `cargo test` invocation indefinitely).
///
/// Spawns `cmd` with piped stdio, hands the `Child` to a worker thread that blocks on
/// `wait_with_output`, and waits on that thread with `recv_timeout`. On timeout, `kill -9`s the PID
/// (best-effort: this may not stop a container the process launched, since `docker run`'s local CLI
/// process is not the containerized process itself, but it does unblock the caller) and panics.
pub fn run_with_timeout(mut cmd: Command, timeout: Duration, test_name: &str) -> Output {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("{test_name}: failed to spawn {cmd:?}: {e}"));
    let pid = child.id();

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(output)) => output,
        Ok(Err(e)) => panic!("{test_name}: process I/O error: {e}"),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            let _ = Command::new("kill").arg("-9").arg(pid.to_string()).status();
            panic!(
                "{test_name}: timed out after {timeout:?} (pid {pid} killed); this suite is \
                 expected to be slow but not THIS slow — check `docker system df` / network"
            );
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            panic!("{test_name}: worker thread died without a result")
        }
    }
}

/// Mirrors [`lci_codegraph::GraphNode`] for `serde::Deserialize` — the crate's own `GraphNode` only
/// derives `Serialize` (it is a write-side/export type), and `container_build.rs` /
/// `container_repos.rs` need to parse `dump-graph`'s JSON stdout back out of a container.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeJson {
    pub node_id: String,
    pub label: String,
    pub source_file: String,
    pub start_line: i64,
}

/// Mirrors [`lci_codegraph::GraphEdge`] for `serde::Deserialize` (see [`NodeJson`]).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeJson {
    pub source: String,
    pub target: String,
    pub relation: String,
}

/// Mirrors [`lci_codegraph::Graph`] for `serde::Deserialize` (see [`NodeJson`]).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GraphJson {
    pub nodes: Vec<NodeJson>,
    pub edges: Vec<EdgeJson>,
}

impl GraphJson {
    /// Every edge endpoint (source and target) must reference a node that actually exists — the
    /// invariant most likely to catch a real resolver bug (a `calls`/`contains`/`method` edge
    /// pointing at a `node_id` nothing emitted).
    pub fn dangling_edges(&self) -> Vec<&EdgeJson> {
        let ids: std::collections::HashSet<&str> =
            self.nodes.iter().map(|n| n.node_id.as_str()).collect();
        self.edges
            .iter()
            .filter(|e| !ids.contains(e.source.as_str()) || !ids.contains(e.target.as_str()))
            .collect()
    }

    /// `node_id`s must be unique — a duplicate means two definitions collapsed onto the same id.
    pub fn duplicate_node_ids(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        let mut dupes = Vec::new();
        for n in &self.nodes {
            if !seen.insert(n.node_id.as_str()) {
                dupes.push(n.node_id.as_str());
            }
        }
        dupes
    }
}
