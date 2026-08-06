//! Minimal CLI used by the container-backed build/repo test suites (`tests/container_build.rs`,
//! `tests/container_repos.rs`) to exercise the crate as a real binary, off the host toolchain,
//! inside glibc/musl containers and against real-world checkouts.
//!
//! Usage: `dump-graph <path-to-checkout>`
//!
//! Walks `<path>` with `build_graph = true` and prints the resulting [`lci_codegraph::Graph`] as
//! JSON to stdout (one line, no pretty-printing — deterministic and cheap to diff across runs) and
//! the [`lci_codegraph::WalkStats`] as a one-line debug dump to stderr. Deliberately dependency-free
//! beyond `lci_codegraph` and `serde_json`, both already crate dependencies.

use std::path::PathBuf;
use std::process::ExitCode;

use lci_codegraph::{WalkOptions, walk_checkout};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let Some(root) = args.next() else {
        eprintln!("usage: dump-graph <path-to-checkout>");
        return ExitCode::FAILURE;
    };
    let root = PathBuf::from(root);

    let options = WalkOptions::builder().build_graph(true).build();
    let output = match walk_checkout(&root, &options) {
        Ok(output) => output,
        Err(error) => {
            eprintln!("dump-graph: walk failed: {error:#}");
            return ExitCode::FAILURE;
        }
    };

    match serde_json::to_string(&output.graph) {
        Ok(json) => println!("{json}"),
        Err(error) => {
            eprintln!("dump-graph: graph serialisation failed: {error:#}");
            return ExitCode::FAILURE;
        }
    }
    eprintln!("{:?}", output.stats);

    ExitCode::SUCCESS
}
