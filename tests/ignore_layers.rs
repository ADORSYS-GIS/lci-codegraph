//! Black-box tests over the two ignore layers `walk_checkout` composes (ADR-0086 §4):
//! - the **operator layer** ([`IgnoreList`]: [`DEFAULT_IGNORE_GLOBS`] + `WalkOptions::extra_ignore_globs`),
//!   applied unconditionally via `filter_entry`;
//! - the **repo layer** (the checkout's own `.gitignore`/`.git/info/exclude`), gated by
//!   `WalkOptions::respect_gitignore` and driven by `ignore::WalkBuilder`'s native git-ignore support.
//!
//! These must **compose**, not replace one another, in both directions, and negation (`!pattern`)
//! must work within each layer.
//!
//! Every ignored/re-admitted file below uses an extension [`lci_codegraph::lang::from_path`]
//! actually recognises (`.rs`, `.txt`, `.json`) — an unrecognised extension (e.g. `.log`) is silently
//! skipped by `walk_checkout` regardless of any ignore rule, which would make an "is it chunked"
//! assertion pass for the wrong reason.

mod common;

use lci_codegraph::{WalkOptions, walk_checkout};

fn chunked(chunks: &[lci_codegraph::Chunk], rel: &str) -> bool {
    chunks.iter().any(|c| c.file_path == rel)
}

#[test]
fn respect_gitignore_true_honours_the_repo_gitignore() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, ".gitignore", "generated/\n");
    common::write(root, "src/a.rs", "fn a() {}\n");
    common::write(root, "generated/b.rs", "fn b() {}\n");

    let options = WalkOptions::builder().respect_gitignore(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(chunked(&out.chunks, "src/a.rs"));
    assert!(!chunked(&out.chunks, "generated/b.rs"));
}

#[test]
fn respect_gitignore_false_walks_repo_gitignored_paths_anyway() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, ".gitignore", "generated/\n");
    common::write(root, "src/a.rs", "fn a() {}\n");
    common::write(root, "generated/b.rs", "fn b() {}\n");

    let options = WalkOptions::builder().respect_gitignore(false).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(chunked(&out.chunks, "src/a.rs"));
    assert!(
        chunked(&out.chunks, "generated/b.rs"),
        "respect_gitignore(false) must walk paths the repo .gitignore would otherwise hide"
    );
}

#[test]
fn respect_gitignore_false_still_honours_the_operator_default_layer() {
    // The operator layer (`DEFAULT_IGNORE_GLOBS` + extra globs) is unconditional — turning off the
    // *repo* .gitignore must not also turn off the operator's own junk-dir defaults.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn a() {}\n");
    common::write(root, "target/generated.rs", "fn g() {}\n");

    let options = WalkOptions::builder().respect_gitignore(false).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(chunked(&out.chunks, "src/a.rs"));
    assert!(
        !chunked(&out.chunks, "target/generated.rs"),
        "operator default target/ must still be pruned even with respect_gitignore(false)"
    );
}

#[test]
fn extra_ignore_globs_compose_with_repo_gitignore_not_replace_it() {
    // Repo .gitignore hides `secret.txt`; the operator adds an independent `*.json` glob. Both must
    // take effect simultaneously — proving the operator layer *adds to*, not *replaces*, the repo's
    // own rules.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, ".gitignore", "secret.txt\n");
    common::write(root, "secret.txt", "top secret\n");
    common::write(root, "debug.json", "{\"noise\": true}\n");
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder()
        .extra_ignore_globs(vec!["*.json".to_string()])
        .build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(chunked(&out.chunks, "src/a.rs"), "unrelated file survives");
    assert!(
        !chunked(&out.chunks, "secret.txt"),
        "repo .gitignore still applies"
    );
    assert!(
        !chunked(&out.chunks, "debug.json"),
        "operator extra glob also applies"
    );
}

#[test]
fn extra_ignore_glob_negation_re_admits_one_file() {
    // A later `!pattern` in the SAME operator layer un-ignores a file an earlier glob in that layer
    // would otherwise hide — ordinary gitignore negation semantics, exercised at the file level (see
    // the sibling test below for why negation can't rescue a file under a *pruned directory*).
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "debug.json", "{\"hide\": true}\n");
    common::write(root, "keep.json", "{\"keep\": true}\n");
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder()
        .extra_ignore_globs(vec!["*.json".to_string(), "!keep.json".to_string()])
        .build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(chunked(&out.chunks, "src/a.rs"));
    assert!(!chunked(&out.chunks, "debug.json"), "still hidden");
    assert!(
        chunked(&out.chunks, "keep.json"),
        "negation re-admits this one file"
    );
}

#[test]
fn negation_cannot_rescue_a_file_under_a_wholly_pruned_directory() {
    // Documents a real, non-obvious interaction: `target/` is a DIRECTORY-level default glob, so
    // `filter_entry` prunes the directory itself and the walk never descends to see the file inside
    // it — a later file-level negation for a path under that directory has nothing left to re-admit.
    // This is standard `ignore`-crate walk behaviour (directory pruning short-circuits descent), not
    // specific to this crate's operator layer, but it's exactly the kind of surprise this fixture
    // pins down.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "target/keep.rs", "fn keep() {}\n");

    let options = WalkOptions::builder()
        .extra_ignore_globs(vec!["!target/keep.rs".to_string()])
        .build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        !chunked(&out.chunks, "target/keep.rs"),
        "a file-level negation cannot rescue a path under a directory the walk never descended into"
    );
}

#[test]
fn repo_gitignore_negation_re_admits_one_file() {
    // The *repo* layer's own negation support (driven entirely by `ignore::WalkBuilder`, not this
    // crate's operator code) — a broad pattern followed by a narrower `!` exception.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, ".gitignore", "*.txt\n!important.txt\n");
    common::write(root, "a.txt", "junk\n");
    common::write(root, "important.txt", "keep me\n");

    let options = WalkOptions::builder().build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(!chunked(&out.chunks, "a.txt"));
    assert!(chunked(&out.chunks, "important.txt"));
}

#[test]
fn include_defaults_is_not_exposed_on_walk_options_defaults_are_always_on() {
    // `WalkOptions` has no knob to disable `DEFAULT_IGNORE_GLOBS` (only `IgnoreConfig` does, one
    // level down) — pin that a plain walk always gets the built-in junk-dir defaults for free.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "node_modules/pkg/index.js", "module.exports = {};\n");
    common::write(root, "src/a.rs", "fn a() {}\n");

    let out = walk_checkout(root, &WalkOptions::builder().build()).unwrap();
    assert!(chunked(&out.chunks, "src/a.rs"));
    assert!(!chunked(&out.chunks, "node_modules/pkg/index.js"));
}
