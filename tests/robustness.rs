//! Robustness against hostile / malformed input the walker cannot control (ADR-0086: a crafted repo
//! must never crash or hang the indexing Job). Every test here asserts the walk *completes* — if any
//! of these panicked, the test would fail on its own via an unwinding panic, so most assertions below
//! are about the SHAPE of the (non-panicking) result, documenting current behaviour rather than
//! prescribing it.

mod common;

use lci_codegraph::{WalkOptions, walk_checkout};

fn chunked(chunks: &[lci_codegraph::Chunk], rel: &str) -> bool {
    chunks.iter().any(|c| c.file_path == rel)
}

#[test]
fn empty_file_does_not_panic_and_produces_no_chunk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/empty.rs", "");
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        !chunked(&out.chunks, "src/empty.rs"),
        "an empty file yields no chunks (not a crash, just nothing to embed)"
    );
    assert!(chunked(&out.chunks, "src/a.rs"));
}

#[test]
fn syntactically_broken_source_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Deliberately invalid Rust: unmatched braces/parens, garbage tokens. Tree-sitter is
    // error-tolerant (parses to an error-laden tree rather than failing outright), and the crate
    // never bails on `root.has_error()` — assert the walk still completes and the rest of the repo is
    // still processed.
    common::write(
        root,
        "src/broken.rs",
        "fn ( { struct !!! )) fn another(a: i32 -> { let x = ;;;\n",
    );
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        chunked(&out.chunks, "src/a.rs"),
        "a broken sibling file must not abort the rest of the walk"
    );
}

#[test]
fn non_utf8_bytes_with_a_source_extension_are_skipped_not_panicked() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // 0xFF 0xFE is not valid UTF-8 in this position — `read_to_string` must fail and the walk skips
    // the file (the `continue` on a read error in `src/walk.rs`), never panicking on the invalid
    // bytes.
    common::write_bytes(root, "src/invalid_utf8.rs", b"fn a() {\xff\xfe}\n");
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        !chunked(&out.chunks, "src/invalid_utf8.rs"),
        "non-UTF8 content is skipped, never chunked"
    );
    assert!(chunked(&out.chunks, "src/a.rs"));
}

#[test]
fn valid_utf8_binary_looking_content_with_a_source_extension_does_not_panic() {
    // A file that is technically valid UTF-8 (NUL is a legal codepoint) but binary-*looking* — the
    // kind of thing a misnamed generated/binary asset could produce, still containing SOME real
    // syntax. Tree-sitter extracts just the `fn a() {}` node by byte range, so the NUL bytes outside
    // it end up in no chunk at all here. See the next test for the case where they do leak through.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut body = vec![0u8, 0u8, 0u8];
    body.extend_from_slice(b"fn a() {}\n");
    body.push(0u8);
    common::write_bytes(root, "src/nulls.rs", &body);
    common::write(root, "src/b.rs", "fn b() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    // The walk must complete regardless of what it decides to do with the NUL-laden file.
    assert!(chunked(&out.chunks, "src/b.rs"));
}

#[test]
fn build_graph_path_rejects_a_nul_laden_blob_exactly_as_chunk_file_does() {
    // REGRESSION TEST for the bug this replaces. `chunk_file` has always rejected binary content by
    // scanning the first 512 bytes for a NUL, but `walk_checkout`'s graph-enabled branch never calls
    // `chunk_file` — it parses the tree itself and, when tree-sitter finds no interesting definitions
    // (as here: no recognisable Rust syntax), falls back straight to `chunk_text` -> `window_chunks`,
    // which had no guard at all. A binary blob that happens to be valid UTF-8 (NUL is a legal
    // codepoint) therefore got windowed with raw NUL bytes in `Chunk::content` — on the ONE path
    // production actually runs. The two entry points now agree: the guard sits where the chunk is
    // produced, so it cannot be bypassed by choosing a different door.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let garbage: Vec<u8> = vec![0u8, 1, 2, 3, 0u8, 5, 6, 0u8];
    common::write_bytes(root, "src/garbage.rs", &garbage);

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        !chunked(&out.chunks, "src/garbage.rs"),
        "binary blob must not be chunked on the build_graph path: {:?}",
        out.chunks
    );
    assert!(
        !out.chunks.iter().any(|c| c.content.contains('\0')),
        "no chunk may carry a raw NUL byte (PostgreSQL `text` rejects it outright)"
    );

    // A skipped binary file is COUNTED, not silently vanished: without this an operator cannot
    // tell "this repo has fewer indexable files than expected" from "a binary asset is misnamed
    // with a source extension", and only the second is actionable.
    assert_eq!(
        out.stats.files_skipped_binary, 1,
        "the binary skip must be visible in IndexStats: {:?}",
        out.stats
    );

    // The two entry points must agree about identical bytes — that agreement is the actual fix.
    let direct = lci_codegraph::chunk_file(
        "src/garbage.rs",
        std::str::from_utf8(&garbage).unwrap(),
        "rust",
        lci_codegraph::IndexTuning::default(),
    );
    assert!(
        direct.is_empty(),
        "chunk_file and the graph-enabled walk must reach the same verdict on the same bytes"
    );
}

#[test]
fn binary_blob_is_kept_out_of_the_graph_too_not_just_the_chunks() {
    // The guard runs in the walk BEFORE either consumer, so a binary blob never reaches the graph
    // builder either. Guarding only inside the chunker would have left tree-sitter parsing garbage
    // into `:Symbol` nodes.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut body = vec![0u8, 0u8];
    // Real, parseable Rust *after* the NUL prefix: without a content-based guard this would emit a
    // perfectly ordinary-looking `victim()` node sourced from a file that is actually binary.
    body.extend_from_slice(b"fn victim() {}\n");
    common::write_bytes(root, "src/blob.rs", &body);
    common::write(root, "src/real.rs", "fn real() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        !out.graph
            .nodes
            .iter()
            .any(|n| n.source_file == "src/blob.rs"),
        "binary file contributed graph nodes: {:?}",
        out.graph.nodes
    );
    // ...and a clean file in the same walk is unaffected.
    assert!(
        out.graph.nodes.iter().any(|n| n.label == "real()"),
        "the clean sibling must still be extracted: {:?}",
        out.graph.nodes
    );
}

#[test]
fn deeply_nested_directories_do_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut rel = String::new();
    for i in 0..60 {
        rel.push_str(&format!("d{i}/"));
    }
    rel.push_str("leaf.rs");
    common::write(root, &rel, "fn leaf() {}\n");

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        chunked(&out.chunks, &rel),
        "a file 60 directories deep is still walked and chunked"
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_to_a_file_is_not_followed_into_a_duplicate_entry() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/real.rs", "fn real() {}\n");
    symlink(root.join("src/real.rs"), root.join("src/link.rs")).unwrap();

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        chunked(&out.chunks, "src/real.rs"),
        "the real file is walked"
    );
    // `ignore::WalkBuilder` does not follow symlinks by default: the symlink entry's file type is
    // neither a regular file nor a directory, so `walk_checkout`'s `entry.file_type().is_file()` gate
    // skips it outright. Documented here as current, non-panicking behaviour.
    assert!(
        !chunked(&out.chunks, "src/link.rs"),
        "a symlink is not followed/chunked by default; chunks = {:?}",
        out.chunks.iter().map(|c| &c.file_path).collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn a_symlink_loop_does_not_hang_or_panic_the_walk() {
    use std::os::unix::fs::symlink;

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn a() {}\n");
    std::fs::create_dir_all(root.join("loop")).unwrap();
    // `loop/self` points back at `loop` itself — a directory symlink cycle. `ignore::WalkBuilder`
    // does not follow directory symlinks by default, so this must complete promptly rather than
    // recursing forever.
    symlink(root.join("loop"), root.join("loop/self")).unwrap();

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(chunked(&out.chunks, "src/a.rs"));
}

#[test]
fn a_utf8_bom_prefixed_file_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut body = vec![0xEF, 0xBB, 0xBF]; // UTF-8 BOM
    body.extend_from_slice(b"fn a() {}\n");
    common::write_bytes(root, "src/bom.rs", &body);

    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    // The BOM bytes are legal UTF-8 (U+FEFF), so `read_to_string` succeeds and the walk proceeds
    // without panicking. Whether the leading BOM confuses the Rust grammar into missing `fn a` is a
    // separate, non-crashing concern this test does not assert either way.
    let _ = out;
}
