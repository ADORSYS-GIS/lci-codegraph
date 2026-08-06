//! Black-box test that [`walk_checkout_from_env`] actually honours the environment variables it
//! documents (`INDEX_*` tuning knobs via [`lci_codegraph::IndexTuning::from_env`], and
//! `LCI_CODEGRAPH_IGNORE_GLOBS` for the operator ignore layer).
//!
//! All env mutation happens inside ONE test function, sequentially, with every variable restored in
//! a `finally`-style guard — `cargo test` runs tests in a single process by default and Rust test
//! threads share the process environment, so setting `INDEX_WINDOW_SIZE` (etc.) from more than one
//! test function would race. No new dependency is pulled in for this; a plain guard struct is enough.

mod common;

use lci_codegraph::walk_checkout_from_env;

/// Restores a set of env vars to their original (possibly absent) values on drop, so a panicking
/// assertion mid-test still cleans up rather than leaking mutated env into whatever test happens to
/// run next in this process.
struct EnvGuard {
    saved: Vec<(&'static str, Option<String>)>,
}

impl EnvGuard {
    fn set(vars: &[(&'static str, &str)]) -> Self {
        let saved = vars
            .iter()
            .map(|(k, _)| (*k, std::env::var(k).ok()))
            .collect();
        for (k, v) in vars {
            unsafe { std::env::set_var(k, v) };
        }
        Self { saved }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (k, v) in &self.saved {
            match v {
                Some(v) => unsafe { std::env::set_var(k, v) },
                None => unsafe { std::env::remove_var(k) },
            }
        }
    }
}

#[test]
fn walk_checkout_from_env_honours_index_tuning_and_ignore_glob_env_vars() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // A 6-line text file: with the DEFAULT window (100/50) this is one single chunk covering every
    // line. Shrinking the window via env must produce more than one chunk — observable proof the env
    // var actually reached the walk, not just that `IndexTuning::from_env`'s own unit test passes in
    // isolation.
    common::write(
        root,
        "notes.txt",
        "line0\nline1\nline2\nline3\nline4\nline5\n",
    );
    // Matched by the operator-glob env var below; a recognised extension (.txt) so the ONLY reason it
    // would be missing from the chunk set is the ignore glob, not "unknown language" (a real trap —
    // see tests/ignore_layers.rs's module doc).
    common::write(root, "hidden.txt", "do not index me\n");

    let guard = EnvGuard::set(&[
        ("INDEX_WINDOW_SIZE", "2"),
        ("INDEX_WINDOW_STEP", "1"),
        ("LCI_CODEGRAPH_IGNORE_GLOBS", "hidden.txt"),
    ]);

    let out = walk_checkout_from_env(root, true).unwrap();

    let notes_chunks: Vec<_> = out
        .chunks
        .iter()
        .filter(|c| c.file_path == "notes.txt")
        .collect();
    assert!(
        notes_chunks.len() > 1,
        "INDEX_WINDOW_SIZE=2 must shrink the window enough to split a 6-line file into more than \
         one chunk; got {notes_chunks:?}"
    );

    assert!(
        !out.chunks.iter().any(|c| c.file_path == "hidden.txt"),
        "LCI_CODEGRAPH_IGNORE_GLOBS must reach the operator ignore layer"
    );

    drop(guard);
}

#[test]
fn walk_checkout_from_env_build_graph_parameter_is_not_env_controlled() {
    // `build_graph` is `walk_checkout_from_env`'s own explicit parameter (unlike the tuning/ignore
    // knobs above, it has no env var) — pin that both values of the parameter are honoured regardless
    // of environment state.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn caller() { target(); }\n");
    common::write(root, "src/b.rs", "fn target() {}\n");

    let off = walk_checkout_from_env(root, false).unwrap();
    assert!(off.graph.nodes.is_empty());

    let on = walk_checkout_from_env(root, true).unwrap();
    assert!(!on.graph.nodes.is_empty());
}
