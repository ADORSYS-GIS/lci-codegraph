//! Integration coverage for the embed step wired into `walk_checkout` (`src/walk.rs`): a real
//! cross-file call graph, resolved once, then embedded against a stub OpenAI-compatible endpoint.
//! `src/embed/context.rs` and `src/embed/client.rs` already cover header-rendering and HTTP-client
//! edge cases in unit tests; this suite is the black-box proof that the two are actually wired
//! together through the public `walk_checkout` API, not exercised only in isolation.

mod common;

use lci_codegraph::{EmbedConfig, WalkOptions, walk_checkout};

/// Arbitrary but fixed — only used to assert every returned vector has the length the stub server
/// was configured to hand back.
const DIMENSION: usize = 4;

#[test]
fn walk_embeds_every_chunk_with_graph_aware_context() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn caller() { target(); }\n");
    common::write(root, "src/b.rs", "fn target() {}\n");

    // A generous request budget: the fixture is two tiny files, so with the default batch size
    // (32) this should be a single request, but the stub doesn't care how many actually land.
    let server = common::EmbedStubServer::start(DIMENSION, 8);
    let embed_config = EmbedConfig::builder().base_url(server.base_url()).build();
    let options = WalkOptions::builder()
        .build_graph(true)
        .embed(embed_config)
        .build();

    let out = walk_checkout(root, &options).unwrap();

    assert!(
        !out.chunks.is_empty(),
        "the fixture must produce at least one chunk"
    );
    assert_eq!(
        out.stats.chunks_embedded,
        out.chunks.len(),
        "every produced chunk must have been embedded"
    );
    assert!(out.stats.embed_batches >= 1);
    for chunk in &out.chunks {
        let embedding = chunk
            .embedding
            .as_ref()
            .unwrap_or_else(|| panic!("chunk {:?} must carry an embedding", chunk.symbol_name));
        assert_eq!(
            embedding.len(),
            DIMENSION,
            "embedding for {:?} has the wrong dimension",
            chunk.symbol_name
        );
    }

    let caller = out
        .chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("caller"))
        .expect("a chunk for the caller() function");
    let caller_input = caller
        .embed_input
        .as_ref()
        .expect("caller chunk must have embed_input set");
    assert!(
        caller_input
            .lines()
            .any(|line| line.starts_with("// calls:") && line.contains("target()")),
        "expected a `// calls:` line naming target(), got:\n{caller_input}"
    );

    let target = out
        .chunks
        .iter()
        .find(|c| c.symbol_name.as_deref() == Some("target"))
        .expect("a chunk for the target() function");
    let target_input = target
        .embed_input
        .as_ref()
        .expect("target chunk must have embed_input set");
    assert!(
        target_input
            .lines()
            .any(|line| line.starts_with("// called by:") && line.contains("caller()")),
        "expected a `// called by:` line naming caller(), got:\n{target_input}"
    );
}

#[test]
fn walk_without_embed_configured_leaves_every_chunk_unembedded() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    common::write(root, "src/a.rs", "fn caller() { target(); }\n");
    common::write(root, "src/b.rs", "fn target() {}\n");

    // `WalkOptions::embed` defaults to `None` — no server started, so a stray dial-out would hang or
    // error rather than silently succeed.
    let options = WalkOptions::builder().build_graph(true).build();
    let out = walk_checkout(root, &options).unwrap();

    assert!(!out.chunks.is_empty());
    assert_eq!(out.stats.chunks_embedded, 0);
    assert_eq!(out.stats.embed_batches, 0);
    assert!(
        out.chunks.iter().all(|c| c.embedding.is_none()),
        "no chunk may carry an embedding when embed is not configured"
    );
    assert!(
        out.chunks.iter().all(|c| c.embed_input.is_none()),
        "embed_input must stay unset when embed is not configured"
    );
}
