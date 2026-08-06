//! Black-box test that [`WalkStats`] counters are accurate against a fixture whose composition is
//! known exactly: 2 source files, 2 operator-default-ignored directories (1 entry pruned each), 1
//! valid PDF, 1 oversized PDF, and 1 malformed PDF.

mod common;

use lci_codegraph::pdf::MAX_PDF_BYTES;
use lci_codegraph::{WalkOptions, walk_checkout};

#[test]
fn walk_stats_match_a_known_composition() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // 2 plain source files → files_chunked += 2.
    common::write(root, "src/a.rs", "fn a() {}\n");
    common::write(root, "src/b.rs", "fn b() {}\n");

    // 2 operator-default-ignored directories, one file each — the directory ENTRY is what gets
    // pruned (and counted), so this contributes exactly 2 to paths_ignored, not more (the walk never
    // descends far enough to see — let alone count — the files inside).
    common::write(root, "target/generated.rs", "fn g() {}\n");
    common::write(root, "node_modules/pkg/index.js", "module.exports = {};\n");

    // 1 valid PDF → pdfs_extracted += 1, files_chunked += 1 (its extracted text is non-empty).
    let pdf = common::minimal_pdf_with_text("Hello LCI");
    common::write_bytes(root, "docs/manual.pdf", &pdf);

    // 1 oversized PDF (skipped pre-parse) + 1 malformed PDF (fails to parse) → pdfs_skipped += 2.
    let huge = vec![b'a'; (MAX_PDF_BYTES + 16) as usize];
    common::write_bytes(root, "docs/huge.pdf", &huge);
    common::write_bytes(root, "docs/broken.pdf", b"%PDF-1.7\nnot a real pdf body");

    let options = WalkOptions::builder()
        .build_graph(true)
        .extract_pdfs(true)
        .build();
    let out = walk_checkout(root, &options).unwrap();

    assert_eq!(
        out.stats.files_chunked, 3,
        "src/a.rs + src/b.rs + docs/manual.pdf"
    );
    assert_eq!(
        out.stats.paths_ignored, 2,
        "the target/ and node_modules/ directory entries, pruned once each"
    );
    assert_eq!(out.stats.pdfs_extracted, 1, "only docs/manual.pdf");
    assert_eq!(
        out.stats.pdfs_skipped, 2,
        "docs/huge.pdf (too large) + docs/broken.pdf (failed to parse)"
    );

    // Cross-check the counters against the actual chunk set, so a future change to the counting
    // logic that *also* miscounts chunks can't slip through by coincidence.
    assert_eq!(
        out.chunks
            .iter()
            .map(|c| c.file_path.as_str())
            .collect::<std::collections::HashSet<_>>()
            .len(),
        3,
        "exactly 3 distinct source files produced chunks"
    );
}
