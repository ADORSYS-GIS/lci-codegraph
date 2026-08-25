//! Semantic embeddings via an OpenAI-compatible `/embeddings` endpoint. There is no local/in-process
//! model and no Cargo feature gate: when [`EmbedConfig::from_env`] finds `OPENAI_BASE_URL` set (or a
//! caller builds an [`EmbedConfig`] directly), embedding is the live path — not a dormant one behind a
//! default-off flag — for [`crate::walk`] via [`embed_output`], and equally for any other reader
//! driving [`crate::input`] directly, since [`embed_output`] takes an
//! [`crate::input::IndexOutput`] rather than anything filesystem-specific.
//!
//! The text sent to the model is **graph-aware**: `context::embed_input` prepends a small
//! deterministic header (enclosing container, callees, callers) derived from the resolved structural
//! [`crate::graph::Graph`] ahead of the chunk's own source, so a chunk that reads as
//! `self.update(&mut acc)` embeds with the calling context that makes it findable by *meaning*, not
//! just by the identifiers that happen to appear in it.

mod client;
mod context;

use crate::chunk::Chunk;
use crate::graph::Graph;
use crate::input::IndexOutput;

use std::time::Duration;

/// Configuration for the embeddings call. `bon` builder (≥3 fields, house idiom).
///
/// `Debug` is hand-written, not derived, so `api_key` can never leak through a `tracing` field, a
/// panic message, or a stray `{:?}` in a log line — see the manual `impl Debug` below.
#[derive(Clone, bon::Builder)]
pub struct EmbedConfig {
    /// OpenAI-compatible API base, e.g. `https://api.openai.com/v1`. A trailing `/` is trimmed when
    /// the request URL (`{base_url}/embeddings`) is built, not when this is stored — so `Clone`/
    /// `Debug` on this struct always show exactly what the caller passed in.
    #[builder(into)]
    pub base_url: String,
    /// Sent as `Authorization: Bearer <key>` only when `Some`. `None` means an unauthenticated
    /// endpoint (a local gateway, vLLM, Ollama's OpenAI shim) — a legitimate configuration, not a
    /// missing one.
    pub api_key: Option<String>,
    /// Embedding model name.
    #[builder(default = default_model())]
    pub model: String,
    /// Passed through as the request's `dimensions` field only when `Some` — omitted rather than
    /// sent as `null`, since not every OpenAI-compatible server accepts either the field or a null.
    /// `Option<T>` fields are optional-by-default under `bon` (default `None`), so no `#[builder]`
    /// attribute is needed here.
    pub dimensions: Option<u32>,
    /// Per-request timeout.
    #[builder(default = Duration::from_secs(30))]
    pub timeout: Duration,
    /// Retries on 429 / 5xx / transport error, exponential backoff starting at 500ms. `0` is legal
    /// (no retry).
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Each input string is truncated to this many **chars** (not bytes) before being sent.
    #[builder(default = 8000)]
    pub max_input_chars: usize,
    /// Max callees and max callers listed in the context header; each side capped independently.
    #[builder(default = 8)]
    pub max_context_refs: usize,
}

fn default_model() -> String {
    "text-embedding-3-small".to_string()
}

impl std::fmt::Debug for EmbedConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbedConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &self.api_key.as_ref().map(|_| "<redacted>"))
            .field("model", &self.model)
            .field("dimensions", &self.dimensions)
            .field("timeout", &self.timeout)
            .field("max_retries", &self.max_retries)
            .field("max_input_chars", &self.max_input_chars)
            .field("max_context_refs", &self.max_context_refs)
            .finish()
    }
}

impl EmbedConfig {
    /// Build from the process environment. `None` when `OPENAI_BASE_URL` is unset or blank — **the**
    /// switch that decides whether a walk embeds at all.
    ///
    /// Note there is deliberately no `OPENAI_EMBEDDING_BATCH_SIZE` here: batch size reuses the
    /// **existing** [`crate::IndexTuning::embed_batch_size`] (`LCI_CODEGRAPH_EMBED_BATCH_SIZE`,
    /// default 32) — that knob already exists for exactly this, and duplicating it would give two
    /// names for one setting.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    /// The testable core of [`Self::from_env`], parameterised over the lookup function so unit tests
    /// never touch the real process environment — see `tests/env_config.rs`'s doc comment for why
    /// that hazard (shared mutable env across test threads in one process) is worth avoiding
    /// entirely here rather than guarding against with a lock.
    pub(crate) fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        let base_url = non_blank(get("OPENAI_BASE_URL"))?;

        // Every optional field is computed up front and set through its `maybe_*` setter, called
        // unconditionally: `bon`'s builder is a typestate machine, so a setter that's called on some
        // branches and skipped on others would produce two different, incompatible builder types —
        // there is no single `builder` variable that could hold both. `maybe_*(None)` and never
        // calling the setter both land on the same default, but only the former keeps the type static.
        let api_key = non_blank(get("OPENAI_API_KEY"));
        let model = non_blank(get("OPENAI_EMBEDDING_MODEL"));
        let dimensions = get("OPENAI_EMBEDDING_DIMENSIONS").and_then(|v| v.trim().parse().ok());
        let timeout = get("OPENAI_EMBEDDING_TIMEOUT_SECS")
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|&secs| secs > 0)
            .map(Duration::from_secs);
        let max_retries = get("OPENAI_EMBEDDING_MAX_RETRIES").and_then(|v| v.trim().parse().ok());
        let max_input_chars = get("OPENAI_EMBEDDING_MAX_INPUT_CHARS")
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|&chars| chars > 0);

        Some(
            Self::builder()
                .base_url(base_url)
                .maybe_api_key(api_key)
                .maybe_model(model)
                .maybe_dimensions(dimensions)
                .maybe_timeout(timeout)
                .maybe_max_retries(max_retries)
                .maybe_max_input_chars(max_input_chars)
                .build(),
        )
    }
}

/// `None`/absent/whitespace-only all collapse to `None` — every env var in [`EmbedConfig::from_lookup`]
/// treats a blank value the same as an unset one.
fn non_blank(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Counters for one [`embed_chunks`] call. Logged by the walk so a slow/rate-limited endpoint (or an
/// operator-misconfigured batch size) is diagnosable.
#[derive(Debug, Default, Clone, Copy)]
pub struct EmbedStats {
    pub chunks_embedded: usize,
    pub batches: usize,
    pub retries: usize,
}

/// Embed every chunk in `chunks` in place, using `graph` for the context header and `config` for
/// endpoint/retry/truncation settings. Batches `batch_size.max(1)` chunks per round-trip (the
/// `.max(1)` guards a `0` reaching here despite [`crate::IndexTuning::from_env`]'s own clamp, the same
/// belt-and-suspenders the windowed chunker applies to its own tuning fields).
///
/// On **any** error this returns `Err` rather than leaving some chunks with `embedding: None` —
/// configured means required here: a half-embedded index that looks complete is worse than a walk
/// that visibly failed, because nothing downstream can tell the difference between "not configured"
/// and "silently failed partway through" from the `Chunk`s alone.
///
/// ```no_run
/// # fn run() -> anyhow::Result<()> {
/// // `EmbedConfig`/`embed_chunks` are addressed through `embed::` here rather than a crate-root
/// // `use`, since re-exporting them at the crate root is wiring this doc example deliberately
/// // doesn't depend on.
/// use lci_codegraph::Graph;
/// use lci_codegraph::embed::EmbedConfig;
///
/// let mut chunks = Vec::new();
/// let graph = Graph::default();
/// let config = EmbedConfig::builder().base_url("https://api.openai.com/v1").build();
/// let stats = lci_codegraph::embed::embed_chunks(&mut chunks, &graph, &config, 32)?;
/// println!("embedded {} chunks in {} batches", stats.chunks_embedded, stats.batches);
/// # Ok(())
/// # }
/// ```
pub fn embed_chunks(
    chunks: &mut [Chunk],
    graph: &Graph,
    config: &EmbedConfig,
    batch_size: usize,
) -> anyhow::Result<EmbedStats> {
    if chunks.is_empty() {
        return Ok(EmbedStats::default());
    }

    // Built once for the whole walk, not per chunk — the point of the index (see `context` module doc).
    let index = context::ContextIndex::build(graph);
    for chunk in chunks.iter_mut() {
        chunk.embed_input = Some(context::embed_input(chunk, &index, config));
    }

    let batch_size = batch_size.max(1);
    let mut stats = EmbedStats::default();
    let mut retries = 0usize;

    for start in (0..chunks.len()).step_by(batch_size) {
        let end = (start + batch_size).min(chunks.len());
        let inputs: Vec<String> = chunks[start..end]
            .iter()
            .map(|c| c.embed_input.clone().unwrap_or_default())
            .collect();

        let vectors = client::embed_batch(config, &inputs, &mut retries)?;
        for (chunk, vector) in chunks[start..end].iter_mut().zip(vectors) {
            chunk.embedding = Some(vector);
        }

        stats.batches += 1;
        stats.chunks_embedded += end - start;
    }

    stats.retries = retries;
    Ok(stats)
}

/// Embed every chunk in `output.chunks` in place, using `output.graph` for the context header, and
/// fold the result into `output.stats.chunks_embedded`/`embed_batches`. The convenience over
/// [`embed_chunks`] for a caller that already has an [`IndexOutput`] in hand — which is any reader at
/// all: `walk_checkout` calls this after `Indexer::finish`, but so can a caller driving
/// `index_inputs`/`Indexer` directly over a tarball, a git object store, or DB rows. That's the point
/// of taking the source-agnostic [`IndexOutput`] rather than a filesystem-walk-specific type —
/// embedding was never actually a filesystem-walk-specific concern, it just needed the indexing
/// core's raw-inputs generalisation (ADR-0086) to stop being implicitly tied to one.
///
/// Deliberately **not** part of [`crate::input::Indexer`] itself: `Indexer::finish` is `#[must_use]`,
/// infallible, and pure CPU work, while embedding is fallible network I/O — the two must not merge
/// into one struct (see `docs/architecture.md`, "Where embedding sits"). This function is the seam:
/// called explicitly, after the indexing core is done, never from inside it.
pub fn embed_output(
    output: &mut IndexOutput,
    config: &EmbedConfig,
    batch_size: usize,
) -> anyhow::Result<EmbedStats> {
    let stats = embed_chunks(&mut output.chunks, &output.graph, config, batch_size)?;
    output.stats.chunks_embedded += stats.chunks_embedded;
    output.stats.embed_batches += stats.batches;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    #[test]
    fn from_lookup_is_none_when_base_url_is_unset() {
        assert!(EmbedConfig::from_lookup(lookup(&[])).is_none());
    }

    #[test]
    fn from_lookup_is_none_when_base_url_is_blank() {
        assert!(EmbedConfig::from_lookup(lookup(&[("OPENAI_BASE_URL", "   ")])).is_none());
    }

    #[test]
    fn from_lookup_applies_defaults_when_only_base_url_is_set() {
        let config =
            EmbedConfig::from_lookup(lookup(&[("OPENAI_BASE_URL", "https://api.openai.com/v1")]))
                .unwrap();
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert!(config.api_key.is_none());
        assert_eq!(config.model, "text-embedding-3-small");
        assert!(config.dimensions.is_none());
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.max_input_chars, 8000);
        assert_eq!(config.max_context_refs, 8);
    }

    #[test]
    fn from_lookup_reads_every_documented_var() {
        let config = EmbedConfig::from_lookup(lookup(&[
            ("OPENAI_BASE_URL", "https://gw.internal/v1"),
            ("OPENAI_API_KEY", "abc123"),
            ("OPENAI_EMBEDDING_MODEL", "custom-model"),
            ("OPENAI_EMBEDDING_DIMENSIONS", "512"),
            ("OPENAI_EMBEDDING_TIMEOUT_SECS", "45"),
            ("OPENAI_EMBEDDING_MAX_RETRIES", "5"),
            ("OPENAI_EMBEDDING_MAX_INPUT_CHARS", "1234"),
        ]))
        .unwrap();
        assert_eq!(config.base_url, "https://gw.internal/v1");
        assert_eq!(config.api_key.as_deref(), Some("abc123"));
        assert_eq!(config.model, "custom-model");
        assert_eq!(config.dimensions, Some(512));
        assert_eq!(config.timeout, Duration::from_secs(45));
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.max_input_chars, 1234);
    }

    #[test]
    fn from_lookup_treats_blank_api_key_as_unset() {
        let config = EmbedConfig::from_lookup(lookup(&[
            ("OPENAI_BASE_URL", "https://x"),
            ("OPENAI_API_KEY", "   "),
        ]))
        .unwrap();
        assert!(config.api_key.is_none());
    }

    #[test]
    fn from_lookup_unparseable_dimensions_is_none_not_an_error() {
        let config = EmbedConfig::from_lookup(lookup(&[
            ("OPENAI_BASE_URL", "https://x"),
            ("OPENAI_EMBEDDING_DIMENSIONS", "not-a-number"),
        ]))
        .unwrap();
        assert!(config.dimensions.is_none());
    }

    #[test]
    fn from_lookup_zero_or_unparseable_timeout_falls_back_to_default() {
        let zero = EmbedConfig::from_lookup(lookup(&[
            ("OPENAI_BASE_URL", "https://x"),
            ("OPENAI_EMBEDDING_TIMEOUT_SECS", "0"),
        ]))
        .unwrap();
        assert_eq!(zero.timeout, Duration::from_secs(30));

        let garbage = EmbedConfig::from_lookup(lookup(&[
            ("OPENAI_BASE_URL", "https://x"),
            ("OPENAI_EMBEDDING_TIMEOUT_SECS", "soon"),
        ]))
        .unwrap();
        assert_eq!(garbage.timeout, Duration::from_secs(30));
    }

    #[test]
    fn from_lookup_zero_max_retries_is_legal() {
        let config = EmbedConfig::from_lookup(lookup(&[
            ("OPENAI_BASE_URL", "https://x"),
            ("OPENAI_EMBEDDING_MAX_RETRIES", "0"),
        ]))
        .unwrap();
        assert_eq!(config.max_retries, 0);
    }

    #[test]
    fn from_lookup_zero_max_input_chars_falls_back_to_default() {
        let config = EmbedConfig::from_lookup(lookup(&[
            ("OPENAI_BASE_URL", "https://x"),
            ("OPENAI_EMBEDDING_MAX_INPUT_CHARS", "0"),
        ]))
        .unwrap();
        assert_eq!(config.max_input_chars, 8000);
    }

    #[test]
    fn debug_output_never_contains_the_api_key() {
        let config = EmbedConfig::builder()
            .base_url("https://x")
            .api_key("super-secret-token".to_string())
            .build();
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("super-secret-token"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn debug_output_shows_none_when_no_api_key_is_set() {
        let config = EmbedConfig::builder().base_url("https://x").build();
        let rendered = format!("{config:?}");
        assert!(rendered.contains("api_key: None"));
    }

    #[test]
    fn embed_chunks_on_empty_input_makes_no_request_and_returns_default_stats() {
        // base_url points nowhere reachable — if this made a real request it would error or hang;
        // returning `Ok(EmbedStats::default())` without dialing out is the whole point of the guard.
        let config = EmbedConfig::builder()
            .base_url("http://127.0.0.1:1")
            .build();
        let graph = Graph::default();
        let mut chunks: Vec<Chunk> = Vec::new();
        let stats = embed_chunks(&mut chunks, &graph, &config, 32).unwrap();
        assert_eq!(stats.chunks_embedded, 0);
        assert_eq!(stats.batches, 0);
        assert_eq!(stats.retries, 0);
    }
}
