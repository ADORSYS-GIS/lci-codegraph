//! The filesystem *reader* (ADR-0086). [`FsSource`] walks a checkout and produces
//! [`crate::input::RawInput`]s, honouring the repo `.gitignore` (composed with the operator ignore
//! layer) and pruning ignored paths before they are ever read. [`walk_checkout`] is a thin driver that
//! feeds an `FsSource` into `crate::input::Indexer` and returns the result — kept as a convenience
//! entry point because it remains the crate's primary caller-facing driver.
//!
//! `FsSource` is one reader among several possible sources for the indexing core in `crate::input`: a
//! git object store, a tarball, an HTTP fetch, editor buffers, or DB rows could each implement their
//! own reader that yields `RawInput`s the same way. The operator ignore layer deliberately lives here,
//! in the reader, rather than in `crate::input` — filtering which inputs are even worth handing over is
//! the reader's job, not the content-level indexer's.
//!
//! This is the crate's top-level entry point; the agent-plane's `index` mode (and today's
//! `agent-runner`, behind a flag) drives it and maps the results onto the internal-API payloads.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use ignore::WalkBuilder;

use crate::embed::{self, EmbedConfig};
use crate::ignore_list::{IgnoreConfig, IgnoreList};
use crate::input::{IndexOptions, IndexOutput, Indexer, MAX_INPUT_BYTES, RawInput};
use crate::tuning::IndexTuning;

/// Options for [`FsSource`] / [`walk_checkout`]. `bon` builder (≥3 fields, ADR-0083 idiom).
#[derive(Debug, Clone, bon::Builder)]
pub struct WalkOptions {
    /// Chunking/window tuning (carried over verbatim, ADR-0086).
    #[builder(default)]
    pub tuning: IndexTuning,
    /// Honour the repo's own `.gitignore` (and nested/parent ignore files). Default true — this is
    /// what makes the operator layer *compose with* rather than *replace* the repo ignores.
    #[builder(default = true)]
    pub respect_gitignore: bool,
    /// Build the in-house structural graph (Rust, Python, TS/JS+TSX, Java). Default `false` so the crate is
    /// behavior-neutral until a host opts in (ADR-0086 migration shape).
    #[builder(default = false)]
    pub build_graph: bool,
    /// Extract text from PDFs and index it. Default true.
    #[builder(default = true)]
    pub extract_pdfs: bool,
    /// Operator-configurable extra ignore globs, layered on top of the built-in defaults.
    #[builder(default)]
    pub extra_ignore_globs: Vec<String>,
    /// When `Some`, embed every chunk against an OpenAI-compatible endpoint after `walk_checkout`
    /// calls `Indexer::finish` (`embed::embed_output`). `None` (the default — `Option<T>` fields are
    /// optional-by-default under `bon`, so no `#[builder]` attribute is needed here) keeps the walk
    /// exactly as behavior-neutral as it was before embedding existed — nothing dials out unless a
    /// caller opts in, either directly or via [`walk_checkout_from_env`]'s `EmbedConfig::from_env`.
    pub embed: Option<EmbedConfig>,
}

/// The FS-reader-specific fields (`respect_gitignore`, `extra_ignore_globs`) have no equivalent in
/// [`IndexOptions`] — they govern which paths `FsSource` hands over, not how content is indexed once
/// handed over. `embed` is left out for the same shape of reason but a different one: it is not a
/// content-level indexing decision at all, it's a `walk_checkout`-only post-processing step that runs
/// *after* `Indexer::finish` returns, so `IndexOptions` — which only ever configures `Indexer` itself —
/// has nothing to plug it into either. This conversion therefore carries only the three fields the two
/// structs genuinely share.
impl From<&WalkOptions> for IndexOptions {
    fn from(options: &WalkOptions) -> Self {
        IndexOptions::builder()
            .tuning(options.tuning)
            .build_graph(options.build_graph)
            .extract_pdfs(options.extract_pdfs)
            .build()
    }
}

/// Reads a filesystem checkout into [`RawInput`]s — the filesystem *reader*, one of several possible
/// sources for [`crate::input::Indexer`]. Applies both ignore layers (repo `.gitignore` + the operator
/// glob list) while walking, so a pruned tree is never descended into and a pruned file is never read.
pub struct FsSource {
    root: PathBuf,
    walk: ignore::Walk,
    ignored: Arc<AtomicUsize>,
}

impl FsSource {
    pub fn new(root: &Path, options: &WalkOptions) -> anyhow::Result<Self> {
        let ignore_list = Arc::new(IgnoreList::build(
            &IgnoreConfig::builder()
                .root(root)
                .extra_globs(options.extra_ignore_globs.clone())
                .build(),
        )?);
        let ignored = Arc::new(AtomicUsize::new(0));

        // Prune the operator-ignored paths (dirs + files) via `filter_entry` so we never descend into a
        // `node_modules`, while `git_ignore` handles the repo's own rules — the two compose.
        let mut builder = WalkBuilder::new(root);
        builder
            .hidden(false) // don't blanket-skip dotfiles; the ignore lists decide
            .git_ignore(options.respect_gitignore)
            .git_global(false) // a machine-global gitignore must not affect indexing determinism
            .git_exclude(options.respect_gitignore)
            .parents(options.respect_gitignore)
            .require_git(false); // honour `.gitignore` even in a non-git fixture dir

        {
            let ignore_list = Arc::clone(&ignore_list);
            let ignored = Arc::clone(&ignored);
            builder.filter_entry(move |entry| {
                let is_dir = entry.file_type().is_some_and(|ft| ft.is_dir());
                if ignore_list.is_ignored(entry.path(), is_dir) {
                    tracing::debug!(path = %entry.path().display(), "codegraph: skipping ignored path");
                    ignored.fetch_add(1, Ordering::Relaxed);
                    return false;
                }
                true
            });
        }

        Ok(Self {
            root: root.to_path_buf(),
            walk: builder.build(),
            ignored,
        })
    }

    /// Paths pruned by the ignore layers so far. Final once iteration is exhausted.
    #[must_use]
    pub fn paths_ignored(&self) -> usize {
        self.ignored.load(Ordering::Relaxed)
    }
}

impl Iterator for FsSource {
    type Item = RawInput<'static>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.walk.next()? {
                Ok(e) => e,
                Err(error) => {
                    tracing::warn!(%error, "codegraph: walk entry error");
                    continue;
                }
            };
            if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }
            let path = entry.path();
            let rel_path = path
                .strip_prefix(&self.root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/");

            // Bound the read at the I/O level, not via `metadata().len()`: the file could grow (or be
            // a pipe/special file) between the stat and the read (TOCTOU), so cap the bytes we
            // actually pull. Read at most MAX_INPUT_BYTES + 1 — the `+ 1` is so `Indexer::push` can
            // still see the cap was exceeded (and count it) rather than silently receiving a
            // truncated input. This is only the outer, reader-side bound: the tighter, per-kind caps
            // (source vs PDF) are applied by `Indexer::push` once it knows which one applies.
            let mut bytes = Vec::new();
            let read = std::fs::File::open(path)
                .and_then(|f| f.take(MAX_INPUT_BYTES as u64 + 1).read_to_end(&mut bytes));
            if read.is_err() {
                continue; // unreadable
            }

            return Some(RawInput::new(rel_path, bytes));
        }
    }
}

/// Walk `root`, producing chunks + (optionally) the structural graph + (optionally) embeddings.
/// Synchronous CPU work — callers on an async runtime should wrap this in `spawn_blocking` (as
/// `agent-runner` does). The one exception to "CPU work" is the embed step below, which is
/// synchronous, fallible network I/O — that's exactly why it runs here, after `Indexer::finish`, and
/// not inside `Indexer` itself (see `docs/architecture.md`, "Where embedding sits").
pub fn walk_checkout(root: &Path, options: &WalkOptions) -> anyhow::Result<IndexOutput> {
    let mut source = FsSource::new(root, options)?;
    let mut indexer = Indexer::new(IndexOptions::from(options));
    for input in &mut source {
        indexer.push(input);
    }
    indexer.record_pruned(source.paths_ignored());
    let mut output = indexer.finish();

    if let Some(embed_config) = &options.embed {
        if !options.build_graph {
            // Not forced on: flipping `build_graph` behind a caller's back would silently change the
            // cost profile (a full parse + resolve pass) of a walk they configured without it.
            tracing::warn!(
                "codegraph: embed configured without build_graph — context headers degrade to \
                 file/language/symbol only, with no within/calls/called-by lines"
            );
        }
        embed::embed_output(&mut output, embed_config, options.tuning.embed_batch_size)?;
    }

    Ok(output)
}

/// Convenience: walk reading tuning + operator ignore globs from the environment
/// (`INDEX_*` and `LCI_CODEGRAPH_IGNORE_GLOBS`) — used by hosts that want env-driven config.
pub fn walk_checkout_from_env(root: &Path, build_graph: bool) -> anyhow::Result<IndexOutput> {
    let options = WalkOptions::builder()
        .tuning(IndexTuning::from_env())
        .build_graph(build_graph)
        .extra_ignore_globs(read_env_globs())
        .maybe_embed(EmbedConfig::from_env())
        .build();
    walk_checkout(root, &options)
}

fn read_env_globs() -> Vec<String> {
    std::env::var(crate::ignore_list::ENV_EXTRA_GLOBS)
        .ok()
        .map(|raw| {
            raw.split(['\n', ','])
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn walk_chunks_and_builds_graph_and_composes_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "src/a.rs", "fn caller() { target(); }\n");
        write(root, "src/b.rs", "fn target() {}\n");
        // Operator-default junk dir: must be skipped even though not in .gitignore.
        write(root, "target/generated.rs", "fn should_not_index() {}\n");
        // Repo .gitignore: composes with the operator layer.
        write(root, ".gitignore", "ignored_by_repo/\n");
        write(root, "ignored_by_repo/secret.rs", "fn secret() {}\n");

        let options = WalkOptions::builder().build_graph(true).build();
        let out = walk_checkout(root, &options).unwrap();

        // Cross-file edge built.
        assert!(
            out.graph.edges.iter().any(|e| e.relation == "calls"),
            "expected a cross-file calls edge, edges = {:?}",
            out.graph.edges
        );
        // Neither the operator-default `target/` nor the repo-.gitignored dir was indexed.
        assert!(
            !out.chunks
                .iter()
                .any(|c| c.file_path.contains("generated.rs")),
            "target/ must be skipped by the operator defaults"
        );
        assert!(
            !out.chunks.iter().any(|c| c.file_path.contains("secret.rs")),
            "repo .gitignore must compose and skip ignored_by_repo/"
        );
        assert!(
            out.stats.paths_ignored >= 1,
            "at least the target/ dir was pruned"
        );
    }

    #[test]
    fn walk_builds_cross_file_graph_for_python_and_ts() {
        // Proves the full walk path — extension detection, `has_graph` gating, the parse-once branch —
        // wires up for the new languages, not just the direct `extract_file` unit tests.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "svc/a.py", "def caller():\n    target()\n");
        write(root, "svc/b.py", "def target():\n    pass\n");
        write(
            root,
            "web/a.ts",
            "import { help } from './b';\nfunction run() { help(); }\n",
        );
        write(root, "web/b.ts", "export function help() {}\n");

        let options = WalkOptions::builder().build_graph(true).build();
        let out = walk_checkout(root, &options).unwrap();

        let cross_file = |lang_dir: &str| {
            out.graph.edges.iter().any(|e| {
                e.relation == "calls"
                    && out
                        .graph
                        .nodes
                        .iter()
                        .find(|n| n.node_id == e.source)
                        .is_some_and(|n| n.source_file.starts_with(lang_dir))
            })
        };
        assert!(
            cross_file("svc/"),
            "python cross-file call edge; edges = {:?}",
            out.graph.edges
        );
        assert!(
            cross_file("web/"),
            "ts cross-file call edge; edges = {:?}",
            out.graph.edges
        );
    }

    #[test]
    fn graph_off_by_default_is_behavior_neutral() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "src/a.rs",
            "fn caller() { target(); }\nfn target() {}\n",
        );
        let out = walk_checkout(root, &WalkOptions::builder().build()).unwrap();
        assert!(out.graph.nodes.is_empty(), "graph off by default");
        assert!(!out.chunks.is_empty(), "chunks still produced");
    }

    #[test]
    fn walk_honours_nested_gitignore_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "keep.rs", "fn keep() {}\n");
        write(root, "sub/.gitignore", "skip.rs\n");
        write(root, "sub/skip.rs", "fn skip() {}\n");
        write(root, "sub/also_keep.rs", "fn also_keep() {}\n");
        let out = walk_checkout(root, &WalkOptions::builder().build()).unwrap();
        assert!(
            !out.chunks.iter().any(|c| c.file_path.contains("skip.rs")),
            "nested .gitignore must be honoured"
        );
        assert!(
            out.chunks
                .iter()
                .any(|c| c.file_path.contains("also_keep.rs")),
            "an un-ignored sibling must still be indexed"
        );
    }

    #[test]
    fn extra_ignore_globs_compose_with_repo_gitignore_not_replace_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, ".gitignore", "repo_ignored/\n");
        write(root, "repo_ignored/a.rs", "fn a() {}\n");
        write(root, "operator_ignored/b.rs", "fn b() {}\n");
        write(root, "keep.rs", "fn keep() {}\n");
        let options = WalkOptions::builder()
            .extra_ignore_globs(vec!["operator_ignored/".to_string()])
            .build();
        let out = walk_checkout(root, &options).unwrap();
        assert!(
            !out.chunks
                .iter()
                .any(|c| c.file_path.contains("repo_ignored")),
            "repo .gitignore must still apply alongside the operator layer"
        );
        assert!(
            !out.chunks
                .iter()
                .any(|c| c.file_path.contains("operator_ignored")),
            "the operator glob must also apply"
        );
        assert!(out.chunks.iter().any(|c| c.file_path.contains("keep.rs")));
    }

    #[test]
    fn extract_pdfs_false_skips_pdf_files_entirely() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "doc.pdf",
            "not actually parsed when extraction is off",
        );
        write(root, "keep.rs", "fn keep() {}\n");
        let options = WalkOptions::builder().extract_pdfs(false).build();
        let out = walk_checkout(root, &options).unwrap();
        assert_eq!(out.stats.pdfs_extracted, 0);
        assert_eq!(out.stats.pdfs_skipped, 0);
        assert!(!out.chunks.iter().any(|c| c.file_path.contains("doc.pdf")));
        assert!(out.chunks.iter().any(|c| c.file_path.contains("keep.rs")));
    }

    #[test]
    fn walk_stats_counters_are_accurate() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "a.rs", "fn a() {}\n");
        write(root, "b.rs", "fn b() {}\n");
        write(root, "target/gen.rs", "fn gen() {}\n"); // operator-default junk dir
        let out = walk_checkout(root, &WalkOptions::builder().build()).unwrap();
        assert_eq!(out.stats.files_chunked, 2, "a.rs + b.rs, not target/gen.rs");
        assert!(
            out.stats.paths_ignored >= 1,
            "at least the target/ dir was pruned"
        );
        assert_eq!(out.stats.pdfs_extracted, 0);
        assert_eq!(out.stats.pdfs_skipped, 0);
    }

    #[test]
    fn non_utf8_file_is_skipped_without_error() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("bad.rs"), [0x66, 0x6e, 0xff, 0xfe, 0x00, 0x62]).unwrap();
        write(root, "good.rs", "fn good() {}\n");
        let out = walk_checkout(root, &WalkOptions::builder().build()).unwrap();
        assert!(!out.chunks.iter().any(|c| c.file_path.contains("bad.rs")));
        assert!(out.chunks.iter().any(|c| c.file_path.contains("good.rs")));
    }

    #[test]
    fn index_options_from_walk_options_carries_only_the_three_shared_fields() {
        // `IndexOptions` has no `respect_gitignore` or `extra_ignore_globs` field — those govern
        // which paths `FsSource` hands over, not how content is indexed once handed over — so the
        // `From` impl must carry exactly `tuning`/`build_graph`/`extract_pdfs` across and be totally
        // unaffected by the two FS-only fields. Proven here by varying ONLY the FS-only fields
        // between two `WalkOptions` and asserting their derived `IndexOptions` are equivalent
        // field-by-field (no `PartialEq` on `IndexOptions`/`IndexTuning`, so this compares by hand).
        let a = WalkOptions::builder()
            .respect_gitignore(true)
            .extra_ignore_globs(vec!["a/".to_string()])
            .build_graph(true)
            .extract_pdfs(false)
            .build();
        let b = WalkOptions::builder()
            .respect_gitignore(false)
            .extra_ignore_globs(vec!["totally-different/".to_string(), "b/".to_string()])
            .build_graph(true)
            .extract_pdfs(false)
            .build();

        let ia = IndexOptions::from(&a);
        let ib = IndexOptions::from(&b);

        assert_eq!(ia.build_graph, ib.build_graph);
        assert_eq!(ia.extract_pdfs, ib.extract_pdfs);
        assert_eq!(ia.tuning.embed_batch_size, ib.tuning.embed_batch_size);
        assert_eq!(ia.tuning.max_chunk_lines, ib.tuning.max_chunk_lines);
        assert_eq!(ia.tuning.window_size, ib.tuning.window_size);
        assert_eq!(ia.tuning.window_step, ib.tuning.window_step);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_files_are_not_indexed_by_default() {
        // `WalkBuilder` defaults to `follow_links(false)`; a symlink's own `file_type()` is neither a
        // regular file nor traversed as a directory, so it must be silently skipped, not indexed.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "target.rs", "fn real() {}\n");
        std::os::unix::fs::symlink(root.join("target.rs"), root.join("link.rs")).unwrap();
        let out = walk_checkout(root, &WalkOptions::builder().build()).unwrap();
        assert_eq!(
            out.stats.files_chunked, 1,
            "only the real file is chunked, not the symlink"
        );
    }
}
