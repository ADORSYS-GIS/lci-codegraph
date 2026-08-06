//! Black-box tests over PDF handling in the full `walk_checkout` path (ADR-0086 §3): a genuinely
//! valid synthesised PDF is extracted + chunked when `extract_pdfs = true` and left untouched when
//! `false`; an oversized or malformed PDF is skipped without panicking or aborting the rest of the
//! walk.

mod common;

use lci_codegraph::pdf::MAX_PDF_BYTES;
use lci_codegraph::{WalkOptions, walk_checkout};

#[test]
fn extract_pdfs_true_extracts_and_chunks_a_valid_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pdf = common::minimal_pdf_with_text("Hello LCI");
    common::write_bytes(root, "docs/manual.pdf", &pdf);
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().extract_pdfs(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert_eq!(out.stats.pdfs_extracted, 1);
    assert_eq!(out.stats.pdfs_skipped, 0);
    let pdf_chunk = out
        .chunks
        .iter()
        .find(|c| c.file_path == "docs/manual.pdf")
        .expect("pdf produced at least one chunk");
    assert!(
        pdf_chunk.content.contains("Hello LCI"),
        "extracted text made it into the chunk content: {:?}",
        pdf_chunk.content
    );
    assert_eq!(pdf_chunk.language, "pdf");
}

#[test]
fn extract_pdfs_false_skips_pdfs_entirely_and_does_not_count_them() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let pdf = common::minimal_pdf_with_text("Hello LCI");
    common::write_bytes(root, "docs/manual.pdf", &pdf);
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().extract_pdfs(false).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(
        !out.chunks.iter().any(|c| c.file_path == "docs/manual.pdf"),
        "pdf must not be chunked when extract_pdfs is off"
    );
    assert_eq!(
        out.stats.pdfs_extracted, 0,
        "not even attempted, so not counted"
    );
    assert_eq!(out.stats.pdfs_skipped, 0);
    assert!(out.chunks.iter().any(|c| c.file_path == "src/a.rs"));
}

#[test]
fn oversized_pdf_is_skipped_without_panicking_or_aborting_the_walk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let blob = vec![b'a'; (MAX_PDF_BYTES + 16) as usize];
    common::write_bytes(root, "docs/huge.pdf", &blob);
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().extract_pdfs(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert_eq!(out.stats.pdfs_extracted, 0);
    assert_eq!(out.stats.pdfs_skipped, 1);
    assert!(
        !out.chunks.iter().any(|c| c.file_path == "docs/huge.pdf"),
        "oversized pdf produces no chunks"
    );
    assert!(
        out.chunks.iter().any(|c| c.file_path == "src/a.rs"),
        "the rest of the walk proceeds despite the oversized pdf"
    );
}

#[test]
fn malformed_pdf_is_skipped_without_panicking_or_aborting_the_walk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write_bytes(
        root,
        "docs/broken.pdf",
        b"%PDF-1.7\nthis is not a real pdf body",
    );
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().extract_pdfs(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert_eq!(out.stats.pdfs_extracted, 0);
    assert_eq!(out.stats.pdfs_skipped, 1);
    assert!(
        !out.chunks.iter().any(|c| c.file_path == "docs/broken.pdf"),
        "malformed pdf produces no chunks"
    );
    assert!(out.chunks.iter().any(|c| c.file_path == "src/a.rs"));
}

#[test]
fn empty_pdf_file_is_skipped_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write_bytes(root, "docs/empty.pdf", b"");
    common::write(root, "src/a.rs", "fn a() {}\n");

    let options = WalkOptions::builder().extract_pdfs(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert_eq!(out.stats.pdfs_skipped, 1);
    assert!(out.chunks.iter().any(|c| c.file_path == "src/a.rs"));
}
