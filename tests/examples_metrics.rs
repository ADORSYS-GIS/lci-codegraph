//! A coverage-metrics matrix over the sample-application gallery (`examples/apps/`), answering one
//! question in numbers rather than in prose: *for each app, in each language, how much of what could
//! be extracted actually was* — and, run to run, whether that number went up or down.
//!
//! `tests/examples.rs` already proves each app's graph is byte-stable; this file is a different axis
//! over the same gallery. A byte-stable graph tells you nothing drifted. It does not tell you whether
//! the gallery, taken as a whole, is getting *better* — more languages graphed, more calls resolved,
//! more framework surface recognised — which is the property the requester actually cares about
//! ("each time a new feature lands, we have the same graph or at least a better graph"). That is a
//! trend over a matrix (examples × languages × adoption), not a single pass/fail, hence a second file
//! rather than another assertion bolted onto the first.
//!
//! ## What "adoption" means here, precisely
//!
//! Four measures, computed once per app and rolled up once per language:
//! - **`call_resolution_rate`** — [`ResolveStats::resolution_rate`], the fraction of call sites the
//!   resolver actually matched to an edge. Lives on [`ResolutionMetrics`] per app and is re-aggregated
//!   per language in [`Metrics::adoption_summary`]. Deliberately NOT the same thing as
//!   `graph.calls_edges` / `graph.nodes` in [`GraphMetrics`]: a route's `route: → handler` edge
//!   (`lci-codegraph-spring`'s `route.rs`) is pushed straight into the graph as an already-resolved
//!   `calls` edge, bypassing `graph::resolve`'s call-site loop entirely — it was found by scanning the
//!   same file, not by resolving a call site, so it can't fail and was never a candidate for the
//!   ambiguous/unresolved buckets either. A Spring app's `calls_edges` therefore runs ahead of its
//!   `calls_resolved` by roughly its route count; both numbers are real, they just answer different
//!   questions ("how many `calls` edges exist" vs "how many parsed call sites did the resolver
//!   itself settle").
//! - **`file_coverage`** — distinct files that produced at least one graph node, divided by the files
//!   whose language even has an extractor ([`AppMetrics::file_coverage`]). Deliberately `Option<f64>`,
//!   `None` rather than `0.0`, when the denominator is zero: a language with no extractor at all (the
//!   Kotlin app is the standing example — `lang::has_graph("kotlin")` is `false` everywhere, so its
//!   `files_graphable` is always `0`) was never *attempted*, and reporting `0%` would misfile it next
//!   to a language that WAS attempted and genuinely extracted nothing. Same reasoning
//!   [`lci_codegraph_model::ResolveStats::resolution_rate`] already applies to a repo with zero call
//!   sites, extended to the one place in this file where the "empty on purpose" case recurs.
//! - **`framework_coverage`** — route + external-service node counts ([`AppMetrics::framework_coverage`]).
//!   Not a rate: there is no meaningful denominator for "how many routes should exist," so this is
//!   reported as a raw count, the same way `graph.route_nodes`/`graph.external_service_nodes` already
//!   are.
//! - **`language_support_tier`** — per language, at the top of [`Metrics`], not per app: whether a
//!   grammar exists, whether a graph extractor exists, whether the (today Spring-only) framework pass
//!   runs over it. The grammar/extractor columns are read straight off [`lang::all`] /
//!   [`lang::has_graph`] — see [`language_support_tiers`] — so this table cannot silently drift from
//!   the registry the way a hand-maintained "supported languages" list could.
//!
//! ## The warn-only drift guard, and its real cost
//!
//! [`examples_coverage_matrix`] never fails on drift, by deliberate instruction: comparing against the
//! committed [`Metrics`] baseline only ever prints a delta table, marking each app `REGRESSED` or
//! `IMPROVED`, to stdout. Say plainly what that buys and what it does not: a warn-only guard *reports*
//! a regression, it does not *prevent* one from landing — a PR whose diff quietly drops a language's
//! resolution rate is still green. The mitigation is deliverable, not a test assertion: `.github/workflows/ci.yml`
//! appends this table to the run's `$GITHUB_STEP_SUMMARY`, so the number is in front of a reviewer on
//! every PR instead of buried in `--nocapture` log output nobody scrolls to. That closes the gap only
//! as far as "somebody has to actually read the summary" closes it — this file does not, and cannot,
//! close it further.
//!
//! What DOES still fail this test: a missing or unparseable committed `examples/metrics.json`, or a
//! `walk_checkout` that returns `Err`. Those are harness failures, not metric drift, and a warn-only
//! contract for the second thing was never meant to cover the first.
//!
//! Regenerate the committed baseline (`examples/metrics.json`, `examples/METRICS.md`) with:
//!
//! ```text
//! UPDATE_GOLDEN=1 cargo test --test examples_metrics
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use lci_codegraph::{WalkOptions, lang, walk_checkout};
use lci_codegraph_model::ResolveStats;
use serde::{Deserialize, Serialize};

// ── The tag-only languages ───────────────────────────────────────────────────────────────────────
//
// `lang::all()` enumerates only the grammar-backed languages (`LanguageSupport` impls). A handful of
// extensions get a language TAG with no grammar behind them at all — chunked via the windowed-text
// fallback, never structurally parsed — and there is no registry to derive that list from; it lives
// as explicit match arms in `lang::from_path` (see that function's own doc comment). Kept here as one
// named constant, rather than re-deriving it ad hoc wherever a "did we see a tag-only language"
// question comes up, and pointed at its one source of truth so a future arm added there is the thing
// that must also update this list.
const TAG_ONLY_LANGUAGES: &[&str] = &["kotlin", "groovy", "go", "c", "cpp", "text"];

/// The only language `Indexer::push` runs the Spring sibling pass over today (`src/input.rs`'s
/// `if language == "java"` gate). Unlike grammar/graph support, framework extraction has no registry
/// entry to derive this from — `docs/design/spring-aware-graph.md` §5.2 draws that boundary on
/// purpose, keeping framework knowledge out of `lang::LanguageSupport` entirely — so this is pinned by
/// hand against the one call site that actually decides it, the same shape of "no registry, so name it
/// once and point at the source of truth" as `TAG_ONLY_LANGUAGES` above.
const FRAMEWORK_PASS_LANGUAGES: &[&str] = &["java"];

// ── Metrics schema ───────────────────────────────────────────────────────────────────────────────
//
// Every collection below is a `BTreeMap`/sorted, never a `HashMap`: `examples/metrics.json` is a
// committed file a `git diff` has to be readable against, so key order must be deterministic across
// runs and across machines, not an artifact of hash-iteration order.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct FileMetrics {
    /// Every file on disk under the app, EXCEPT `graph.json`, `metrics.json`, and `README.md`. Those
    /// three are this crate's own generated/documentation artifacts, not part of the sample
    /// application — counting them would inflate `files_total` and, worse, misclassify `README.md` as
    /// a "text"-language source file that was never actually part of the app under test, distorting
    /// every per-language ratio derived from it.
    files_total: usize,
    files_chunked: usize,
    files_skipped_unsupported: usize,
    /// Files whose detected language has a graph extractor (`lang::has_graph`) — the denominator of
    /// `file_coverage` below.
    files_graphable: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct GraphMetrics {
    nodes: usize,
    edges: usize,
    calls_edges: usize,
    /// Nodes whose `node_id` starts `route:` (`tests/examples.rs`'s own convention for finding them).
    route_nodes: usize,
    /// Nodes whose `node_id` starts `service:` — an external-service boundary marker (e.g. a
    /// `@FeignClient`), never a local definition.
    external_service_nodes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct ResolutionMetrics {
    calls_resolved: usize,
    calls_ambiguous: usize,
    calls_unresolved: usize,
    /// [`ResolveStats::resolution_rate`] over this app's three counters — computed through that shared
    /// type rather than reimplemented here, so this file can never define "resolution rate" any
    /// differently than the resolver that produced the counters.
    resolution_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct LanguageInApp {
    /// Files on disk (see [`FileMetrics::files_total`]'s exclusions) detected as this language.
    files: usize,
    has_grammar: bool,
    has_graph: bool,
    /// Graph nodes whose `source_file` detects as this language.
    graph_nodes: usize,
    /// DISTINCT files of this language that produced at least one graph node — the numerator this
    /// app contributes to this language's `file_coverage` in [`Metrics::adoption_summary`]. Kept
    /// separate from `graph_nodes` (a node count, not a file count) because file coverage is a
    /// per-file measure: several nodes commonly share one file, and reusing the node count here would
    /// silently inflate the aggregate coverage ratio.
    graph_files: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct AppMetrics {
    files: FileMetrics,
    graph: GraphMetrics,
    resolution: ResolutionMetrics,
    /// Keyed by language tag, one entry per language tag actually present in this app (not every
    /// language the crate knows about).
    languages: BTreeMap<String, LanguageInApp>,
    /// Distinct files (of ANY language) appearing as a `source_file` on some graph node, divided by
    /// `files.files_graphable`. `None`, not `0.0`, when `files_graphable == 0` — see this file's own
    /// module doc for why that distinction is load-bearing.
    file_coverage: Option<f64>,
    /// `graph.route_nodes + graph.external_service_nodes` — how much framework-aware surface this app
    /// exercises. Stored rather than left implicit so a consumer of `metrics.json` doesn't have to
    /// recompute it to answer "does this app show any framework awareness at all".
    framework_coverage: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct LanguageSupportTier {
    has_grammar: bool,
    has_graph: bool,
    framework_pass: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct LanguageAdoptionSummary {
    /// Apps in the gallery containing at least one file of this language.
    apps: usize,
    /// [`ResolveStats::resolution_rate`] over this language's calls, summed across every app where
    /// this language is graphable — sound only because each app in the gallery carries at most one
    /// graphable language at a time (asserted in [`compute_app_metrics`]); see that assertion for what
    /// would have to change if that ever stops being true.
    call_resolution_rate: f64,
    /// Total call sites recorded across this language's apps. Kept so the rendered table can
    /// distinguish "no call site was ever recorded" (no grammar — nothing attempted) from
    /// "call sites existed and none resolved", which `call_resolution_rate == 0.0` alone cannot.
    call_sites: usize,
    file_coverage: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Metrics {
    /// Keyed by app directory name under `examples/apps/`.
    apps: BTreeMap<String, AppMetrics>,
    /// Keyed by language tag, union of every tag observed across `apps` — a coverage report over the
    /// gallery, not a general capability listing over the whole crate's language registry, so a
    /// grammar the gallery never exercises (e.g. plain JavaScript) does not appear here.
    languages: BTreeMap<String, LanguageSupportTier>,
    adoption_summary: BTreeMap<String, LanguageAdoptionSummary>,
}

// ── Computing one app's metrics ──────────────────────────────────────────────────────────────────

fn apps_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/apps")
}

fn app_root(name: &str) -> PathBuf {
    apps_root().join(name)
}

fn metrics_json_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/metrics.json")
}

fn metrics_md_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/METRICS.md")
}

/// The app directory names under `examples/apps/`, discovered from disk rather than hardcoded —
/// deliberately, so a new app dropped into the gallery is picked up here automatically instead of
/// needing a second, parallel list of app names kept in sync with the directory listing by hand (the
/// exact duplicate-hardcoded-list failure mode that bites release pipelines: one copy gets updated,
/// the other goes stale silently).
fn app_names() -> Vec<String> {
    let root = apps_root();
    let mut names: Vec<String> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", root.display()))
        .map(|entry| entry.unwrap_or_else(|e| panic!("dir entry under {}: {e}", root.display())))
        .filter(|entry| entry.file_type().is_ok_and(|ft| ft.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
}

/// Every file on disk under `root`, '/'-separated and relative to `root`, EXCLUDING `graph.json`,
/// `metrics.json`, `README.md` — see [`FileMetrics::files_total`] for why those three are excluded.
fn list_app_files(root: &Path) -> Vec<String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files);
    files.sort();
    files
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let entry = entry.unwrap_or_else(|e| panic!("dir entry under {}: {e}", dir.display()));
        let path = entry.path();
        let file_type = entry
            .file_type()
            .unwrap_or_else(|e| panic!("file_type for {}: {e}", path.display()));
        if file_type.is_dir() {
            collect_files(root, &path, out);
            continue;
        }
        if !file_type.is_file() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if matches!(file_name, "graph.json" | "metrics.json" | "README.md") {
            continue;
        }
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(rel);
    }
}

/// The three filenames excluded from every metric in this file — see [`FileMetrics::files_total`].
/// Fed to the walker as `extra_ignore_globs` (a bare filename, no `/`, matches that basename at any
/// depth under gitignore semantics) rather than filtered out of its output after the fact: pruning
/// them BEFORE `Indexer::push` ever sees them keeps `IndexStats`'s own counters (`files_chunked`,
/// `files_skipped_unsupported`, …) scoped to the same file set [`list_app_files`] enumerates, so the
/// two can be compared directly instead of needing a correction factor for artifacts one side counts
/// and the other doesn't.
const EXCLUDED_ARTIFACTS: &[&str] = &["graph.json", "metrics.json", "README.md"];

fn compute_app_metrics(app: &str) -> AppMetrics {
    let options = WalkOptions::builder()
        .build_graph(true)
        .extra_ignore_globs(EXCLUDED_ARTIFACTS.iter().map(|s| s.to_string()).collect())
        .build();
    let out = walk_checkout(&app_root(app), &options)
        .unwrap_or_else(|e| panic!("walk sample app {app}: {e}"));

    let files = list_app_files(&app_root(app));

    // Sanity check that this test's own directory listing agrees with the crate's own walker about
    // what counts as a file here. `paths_ignored` should equal exactly the artifact files pruned by
    // `extra_ignore_globs` above (0, 1, 2, or 3, depending which of `graph.json`/`metrics.json`/
    // `README.md` exist for this app) — anything else means an operator-default ignore glob
    // (`target/`, `node_modules/`, …) unexpectedly pruned something in what is supposed to be a
    // clean, hand-built checkout, which should fail loudly rather than silently skew every count
    // below.
    let expected_ignored = EXCLUDED_ARTIFACTS
        .iter()
        .filter(|name| app_root(app).join(name).is_file())
        .count();
    assert_eq!(
        out.stats.paths_ignored, expected_ignored,
        "{app}: expected exactly the {expected_ignored} excluded-artifact file(s) to be pruned, got \
         {} ignored paths — investigate before trusting files_total against the walker's own \
         accounting",
        out.stats.paths_ignored
    );
    let accounted_for = out.stats.files_chunked
        + out.stats.files_skipped_unsupported
        + out.stats.files_skipped_binary
        + out.stats.files_skipped_too_large
        + out.stats.pdfs_extracted
        + out.stats.pdfs_skipped;
    // A zero-byte file (e.g. Python's conventional empty `__init__.py`) decodes as UTF-8 fine, isn't
    // binary content, isn't oversized — but produces zero chunks, so `Indexer::push`'s
    // `if !file_chunks.is_empty()` guard never increments `files_chunked`. It already cleared every
    // skip check too, so it lands in none of `IndexStats`' counters at all: a real, pre-existing gap
    // in the crate's own accounting (not something introduced here), named explicitly rather than
    // silently working around it by loosening the assertion below into meaninglessness.
    let empty_files = files
        .iter()
        .filter(|rel| {
            fs::metadata(app_root(app).join(rel))
                .map(|m| m.len() == 0)
                .unwrap_or(false)
        })
        .count();
    assert_eq!(
        files.len(),
        accounted_for + empty_files,
        "{app}: files_total ({}) must equal the sum of every bucket the walker sorted its inputs into \
         ({accounted_for}) plus the {empty_files} zero-byte file(s) IndexStats can't see — a mismatch \
         beyond that means this test's own directory listing and the crate's FsSource disagree about \
         what counts as a file here",
        files.len()
    );

    // ── Per-language file counts, from disk ─────────────────────────────────────────────────────
    let mut lang_files: BTreeMap<String, usize> = BTreeMap::new();
    for rel in &files {
        if let Some(lang_id) = lang::from_path(Path::new(rel)) {
            *lang_files.entry(lang_id.to_string()).or_default() += 1;
        }
    }

    // ── Per-language graph contribution, from the graph itself ──────────────────────────────────
    let mut lang_graph_nodes: BTreeMap<String, usize> = BTreeMap::new();
    let mut lang_graph_files: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for node in &out.graph.nodes {
        if let Some(lang_id) = lang::from_path(Path::new(&node.source_file)) {
            *lang_graph_nodes.entry(lang_id.to_string()).or_default() += 1;
            lang_graph_files
                .entry(lang_id.to_string())
                .or_default()
                .insert(node.source_file.clone());
        }
    }

    let languages: BTreeMap<String, LanguageInApp> = lang_files
        .iter()
        .map(|(lang_id, &file_count)| {
            (
                lang_id.clone(),
                LanguageInApp {
                    files: file_count,
                    has_grammar: lang::all().iter().any(|l| l.id() == lang_id),
                    has_graph: lang::has_graph(lang_id),
                    graph_nodes: lang_graph_nodes.get(lang_id).copied().unwrap_or(0),
                    graph_files: lang_graph_files.get(lang_id).map_or(0, BTreeSet::len),
                },
            )
        })
        .collect();

    // `graph::resolve`'s three counters are whole-app, not per-language — there is no way to ask "how
    // many of these calls came from language X" from `IndexStats` alone. Attributing an app's whole
    // resolve-stats to one language (in `adoption_summary`, built by the caller) is therefore only
    // sound when at most one graphable language is present per app. True of every app in this gallery
    // today (`examples/README.md`: one framework/language stack per checkout) — asserted here, not
    // merely assumed, so a future app mixing two graphable languages fails LOUDLY instead of silently
    // mis-attributing its calls to whichever language happens to sort first.
    let graphable_present: Vec<&str> = languages
        .iter()
        .filter(|(_, l)| l.has_graph && l.files > 0)
        .map(|(id, _)| id.as_str())
        .collect();
    assert!(
        graphable_present.len() <= 1,
        "{app}: {} graphable languages present at once ({graphable_present:?}) — adoption_summary \
         attributes a whole app's resolve-stats to its one graphable language, and that assumption \
         just broke; redesign the attribution before trusting these numbers",
        graphable_present.len()
    );

    let files_graphable: usize = languages
        .values()
        .filter(|l| l.has_graph)
        .map(|l| l.files)
        .sum();

    let distinct_files_in_graph: usize = out
        .graph
        .nodes
        .iter()
        .map(|n| n.source_file.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    let file_coverage = if files_graphable == 0 {
        None
    } else {
        Some(distinct_files_in_graph as f64 / files_graphable as f64)
    };

    let route_nodes = out
        .graph
        .nodes
        .iter()
        .filter(|n| n.node_id.starts_with("route:"))
        .count();
    let external_service_nodes = out
        .graph
        .nodes
        .iter()
        .filter(|n| n.node_id.starts_with("service:"))
        .count();
    let calls_edges = out
        .graph
        .edges
        .iter()
        .filter(|e| e.relation == "calls")
        .count();

    let resolve_stats = ResolveStats {
        calls_resolved: out.stats.calls_resolved,
        calls_ambiguous: out.stats.calls_ambiguous,
        calls_unresolved: out.stats.calls_unresolved,
    };

    AppMetrics {
        files: FileMetrics {
            files_total: files.len(),
            files_chunked: out.stats.files_chunked,
            files_skipped_unsupported: out.stats.files_skipped_unsupported,
            files_graphable,
        },
        graph: GraphMetrics {
            nodes: out.graph.nodes.len(),
            edges: out.graph.edges.len(),
            calls_edges,
            route_nodes,
            external_service_nodes,
        },
        resolution: ResolutionMetrics {
            calls_resolved: resolve_stats.calls_resolved,
            calls_ambiguous: resolve_stats.calls_ambiguous,
            calls_unresolved: resolve_stats.calls_unresolved,
            resolution_rate: resolve_stats.resolution_rate(),
        },
        languages,
        file_coverage,
        framework_coverage: route_nodes + external_service_nodes,
    }
}

/// The top-level `languages` support-tier table: union of every language tag observed across `apps`.
/// The grammar/graph columns come straight off `lang::all()`/`lang::has_graph` — the registry — so
/// they cannot drift from what the crate actually ships the way a hand-maintained list could;
/// `framework_pass` has no registry to read from (see [`FRAMEWORK_PASS_LANGUAGES`]) and is pinned by
/// hand instead.
fn language_support_tiers(
    apps: &BTreeMap<String, AppMetrics>,
) -> BTreeMap<String, LanguageSupportTier> {
    let mut languages = BTreeMap::new();
    for app_metrics in apps.values() {
        for lang_id in app_metrics.languages.keys() {
            languages
                .entry(lang_id.clone())
                .or_insert_with(|| LanguageSupportTier {
                    has_grammar: lang::all().iter().any(|l| l.id() == lang_id),
                    has_graph: lang::has_graph(lang_id),
                    framework_pass: FRAMEWORK_PASS_LANGUAGES.contains(&lang_id.as_str()),
                });
        }
    }
    languages
}

/// Aggregate `call_resolution_rate` and `file_coverage` per language across every app that carries it.
/// For a graphable language this sums the (whole-app) resolve counters and graphable/covered file
/// counts from every app where it is present — sound per the assertion in [`compute_app_metrics`]. For
/// a language with no extractor at all (`lang::has_graph` false, e.g. Kotlin), every app's
/// contribution is trivially zero calls and zero graphable files, which is exactly the `files_graphable
/// == 0` case `AppMetrics::file_coverage` already treats as `None` rather than `0.0` — carried through
/// here for the same reason: a language that was never attempted must not read like one that was tried
/// and failed.
fn adoption_summary(
    apps: &BTreeMap<String, AppMetrics>,
    languages: &BTreeMap<String, LanguageSupportTier>,
) -> BTreeMap<String, LanguageAdoptionSummary> {
    languages
        .keys()
        .map(|lang_id| {
            let apps_with_lang: Vec<&AppMetrics> = apps
                .values()
                .filter(|m| m.languages.contains_key(lang_id))
                .collect();

            let (resolved, ambiguous, unresolved, files_graphable, graph_files) =
                if lang::has_graph(lang_id) {
                    apps_with_lang
                        .iter()
                        .fold((0, 0, 0, 0, 0), |(r, a, u, fg, gf), m| {
                            let l = &m.languages[lang_id];
                            (
                                r + m.resolution.calls_resolved,
                                a + m.resolution.calls_ambiguous,
                                u + m.resolution.calls_unresolved,
                                fg + m.files.files_graphable,
                                gf + l.graph_files,
                            )
                        })
                } else {
                    (0, 0, 0, 0, 0)
                };

            let resolve_stats = ResolveStats {
                calls_resolved: resolved,
                calls_ambiguous: ambiguous,
                calls_unresolved: unresolved,
            };
            let file_coverage = if files_graphable == 0 {
                None
            } else {
                Some(graph_files as f64 / files_graphable as f64)
            };

            (
                lang_id.clone(),
                LanguageAdoptionSummary {
                    apps: apps_with_lang.len(),
                    call_resolution_rate: resolve_stats.resolution_rate(),
                    call_sites: resolve_stats.call_sites(),
                    file_coverage,
                },
            )
        })
        .collect()
}

fn compute_metrics() -> Metrics {
    let apps: BTreeMap<String, AppMetrics> = app_names()
        .into_iter()
        .map(|name| {
            let metrics = compute_app_metrics(&name);
            (name, metrics)
        })
        .collect();
    let languages = language_support_tiers(&apps);
    let adoption_summary = adoption_summary(&apps, &languages);
    Metrics {
        apps,
        languages,
        adoption_summary,
    }
}

// ── Rendering: METRICS.md ────────────────────────────────────────────────────────────────────────

fn fmt_pct(rate: f64) -> String {
    format!("{:.1}%", rate * 100.0)
}

/// A resolution rate is only meaningful when there were call sites to resolve. With none, render
/// `n/a` rather than `0.0%`.
///
/// This is the same trap `file_coverage` avoids by being `Option`, in a different column: `0.0%`
/// reads as "we tried to resolve this app's calls and got none of them", when the truth for
/// `spring-boot-gradle-kotlin` is that it has no grammar, so no call site was ever recorded and
/// nothing was attempted. `ResolveStats::resolution_rate` returning `0.0` for an empty run is
/// correct for aggregation — it keeps sums from becoming NaN — but propagating that `0.0` into a
/// human-facing matrix would state a failure that did not happen.
fn fmt_rate(resolution: &ResolutionMetrics) -> String {
    // Sum the three buckets rather than storing a fourth field: they are mutually exclusive and
    // exhaustive over recorded call sites (`ResolveStats::call_sites`), so a stored total could only
    // ever drift from them.
    if resolution.calls_resolved + resolution.calls_ambiguous + resolution.calls_unresolved == 0 {
        return "n/a (no call sites)".to_string();
    }
    fmt_pct(resolution.resolution_rate)
}

fn fmt_opt_pct(rate: Option<f64>) -> String {
    match rate {
        Some(r) => fmt_pct(r),
        None => "n/a (no graphable files)".to_string(),
    }
}

fn render_metrics_md(metrics: &Metrics) -> String {
    let mut out = String::new();
    out.push_str(
        "# Example gallery coverage matrix\n\n\
         GENERATED by `tests/examples_metrics.rs` — never hand-edit this file. Regenerate with:\n\n\
         ```text\n\
         UPDATE_GOLDEN=1 cargo test --test examples_metrics\n\
         ```\n\n\
         See that file's own module doc for what each column means and why `file_coverage` is `null`\n\
         rather than `0%` when a language has no extractor at all.\n\n",
    );

    out.push_str("## Examples × graph metrics\n\n");
    out.push_str(
        "| App | Nodes | Edges | Calls | Routes | Services | Resolution rate | File coverage |\n",
    );
    out.push_str("|---|---:|---:|---:|---:|---:|---:|---:|\n");
    for (app, m) in &metrics.apps {
        out.push_str(&format!(
            "| {app} | {} | {} | {} | {} | {} | {} | {} |\n",
            m.graph.nodes,
            m.graph.edges,
            m.graph.calls_edges,
            m.graph.route_nodes,
            m.graph.external_service_nodes,
            fmt_rate(&m.resolution),
            fmt_opt_pct(m.file_coverage),
        ));
    }

    out.push_str("\n## Examples × languages\n\n");
    out.push_str(
        "Incidence matrix: file count per language, per app. A blank cell means that language does \
         not appear in that app at all.\n\n",
    );
    out.push_str("| App |");
    for lang_id in metrics.languages.keys() {
        out.push_str(&format!(" {lang_id} |"));
    }
    out.push('\n');
    out.push_str("|---|");
    for _ in metrics.languages.keys() {
        out.push_str("---:|");
    }
    out.push('\n');
    for (app, m) in &metrics.apps {
        out.push_str(&format!("| {app} |"));
        for lang_id in metrics.languages.keys() {
            match m.languages.get(lang_id) {
                Some(l) => out.push_str(&format!(" {} |", l.files)),
                None => out.push_str(" |"),
            }
        }
        out.push('\n');
    }

    out.push_str("\n## Languages × capability\n\n");
    out.push_str("| Language | Grammar | Graph extractor | Framework pass | What you get |\n");
    out.push_str("|---|:---:|:---:|:---:|---|\n");
    for (lang_id, tier) in &metrics.languages {
        let what = match (tier.has_graph, tier.framework_pass) {
            (true, true) => "structural graph + framework-aware routes/boundaries",
            (true, false) => "structural graph (definitions, cross-file calls)",
            (false, _) if tier.has_grammar => "structured chunking, no graph",
            (false, _) if TAG_ONLY_LANGUAGES.contains(&lang_id.as_str()) => {
                "windowed-text chunking only"
            }
            (false, _) => "unclassified",
        };
        out.push_str(&format!(
            "| {lang_id} | {} | {} | {} | {what} |\n",
            if tier.has_grammar { "yes" } else { "no" },
            if tier.has_graph { "yes" } else { "no" },
            if tier.framework_pass { "yes" } else { "no" },
        ));
    }

    out.push_str("\n## Adoption summary\n\n");
    out.push_str(
        "Resolution rate and file coverage, aggregated per language across every app that carries \
         it.\n\n",
    );
    out.push_str("| Language | Apps | Call resolution rate | File coverage |\n");
    out.push_str("|---|---:|---:|---:|\n");
    for (lang_id, summary) in &metrics.adoption_summary {
        out.push_str(&format!(
            "| {lang_id} | {} | {} | {} |\n",
            summary.apps,
            if summary.call_sites == 0 {
                "n/a (no call sites)".to_string()
            } else {
                fmt_pct(summary.call_resolution_rate)
            },
            fmt_opt_pct(summary.file_coverage),
        ));
    }

    out
}

// ── The drift report ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drift {
    New,
    Removed,
    Regressed,
    Improved,
    Unchanged,
}

impl Drift {
    fn label(self) -> &'static str {
        match self {
            Drift::New => "NEW",
            Drift::Removed => "REMOVED",
            Drift::Regressed => "REGRESSED",
            Drift::Improved => "IMPROVED",
            Drift::Unchanged => "unchanged",
        }
    }
}

/// Tolerance for comparing two `resolution_rate` floats across a JSON round trip. `serde_json` 1.0.150
/// does not parse every float back to the exact same bit pattern `f64::to_string` would have produced
/// (confirmed empirically: `"0.18421052631578946".parse::<f64>()` and
/// `serde_json::from_str::<f64>("0.18421052631578946")` disagree in the last bit) — a strict `<`/`>`
/// comparison between a freshly computed rate and one that came back through `examples/metrics.json`
/// would flag that 1-ULP artifact as REGRESSED/IMPROVED even when the underlying integer counters
/// (`calls_resolved`/`calls_ambiguous`/`calls_unresolved`) are bit-for-bit identical. `1e-9` is many
/// orders of magnitude below any 1-ULP float noise yet far below any rate change a single call site
/// could produce even in the largest app in this gallery, so it cannot mask a genuine regression.
const RESOLUTION_RATE_EPSILON: f64 = 1e-9;

/// `REGRESSED` if any of the three named signals got worse (fewer resolved calls, a lower resolution
/// rate, fewer nodes) — checked first and wins outright even if another signal improved at the same
/// time, because "a language extractor lost nodes" is worth flagging on its own even when some other
/// number happens to look better. `IMPROVED` only once none of those three got worse and at least one
/// got better; otherwise `Unchanged`.
fn classify_drift(old: &AppMetrics, new: &AppMetrics) -> Drift {
    let regressed = new.resolution.calls_resolved < old.resolution.calls_resolved
        || new.resolution.resolution_rate
            < old.resolution.resolution_rate - RESOLUTION_RATE_EPSILON
        || new.graph.nodes < old.graph.nodes;
    if regressed {
        return Drift::Regressed;
    }
    let improved = new.resolution.calls_resolved > old.resolution.calls_resolved
        || new.resolution.resolution_rate
            > old.resolution.resolution_rate + RESOLUTION_RATE_EPSILON
        || new.graph.nodes > old.graph.nodes;
    if improved {
        Drift::Improved
    } else {
        Drift::Unchanged
    }
}

fn print_matrix(baseline: Option<&Metrics>, current: &Metrics) {
    println!("BEGIN MATRIX");
    println!("## Example coverage matrix (examples/apps/*)\n");
    println!("| App | Nodes | Edges | Calls resolved | Resolution rate | File coverage | Drift |");
    println!("|---|---:|---:|---:|---:|---:|---|");

    let mut app_names: BTreeSet<&String> = current.apps.keys().collect();
    if let Some(baseline) = baseline {
        app_names.extend(baseline.apps.keys());
    }
    for app in app_names {
        let new = current.apps.get(app);
        let old = baseline.and_then(|b| b.apps.get(app));
        let drift = match (old, new) {
            (Some(old), Some(new)) => classify_drift(old, new),
            (None, Some(_)) => Drift::New,
            (Some(_), None) => Drift::Removed,
            (None, None) => unreachable!("app came from one of the two maps we just unioned"),
        };
        let row = new.or(old).expect("at least one side has this app");
        println!(
            "| {app} | {} | {} | {} | {} | {} | {} |",
            row.graph.nodes,
            row.graph.edges,
            row.resolution.calls_resolved,
            fmt_rate(&row.resolution),
            fmt_opt_pct(row.file_coverage),
            drift.label(),
        );
    }

    println!("\n## Adoption summary (aggregated per language)\n");
    println!("| Language | Apps | Call resolution rate | File coverage |");
    println!("|---|---:|---:|---:|");
    for (lang_id, summary) in &current.adoption_summary {
        println!(
            "| {lang_id} | {} | {} | {} |",
            summary.apps,
            fmt_pct(summary.call_resolution_rate),
            fmt_opt_pct(summary.file_coverage),
        );
    }
    println!("END MATRIX");
}

// ── The test ──────────────────────────────────────────────────────────────────────────────────────

#[test]
fn examples_coverage_matrix() {
    let current = compute_metrics();

    if std::env::var("UPDATE_GOLDEN").is_ok() {
        let json = serde_json::to_string_pretty(&current).expect("metrics serialise");
        std::fs::write(metrics_json_path(), format!("{json}\n")).unwrap();
        std::fs::write(metrics_md_path(), render_metrics_md(&current)).unwrap();
        eprintln!(
            "wrote {} and {}",
            metrics_json_path().display(),
            metrics_md_path().display()
        );
        print_matrix(None, &current);
        return;
    }

    // Harness failures — as opposed to metric drift — still fail this test. A missing or unparseable
    // baseline means the drift report below has nothing honest to compare against.
    let baseline_json = std::fs::read_to_string(metrics_json_path()).unwrap_or_else(|e| {
        panic!(
            "read {} ({e}); regenerate with UPDATE_GOLDEN=1 cargo test --test examples_metrics",
            metrics_json_path().display()
        )
    });
    let baseline: Metrics = serde_json::from_str(&baseline_json).unwrap_or_else(|e| {
        panic!(
            "parse {} ({e}); it is either corrupt or predates this schema — regenerate with \
             UPDATE_GOLDEN=1 cargo test --test examples_metrics",
            metrics_json_path().display()
        )
    });

    // Drift is warn-only from here on — see this file's module doc for why, and what that tradeoff
    // costs. Nothing below this line may call `panic!`/`assert!` over a metrics difference.
    print_matrix(Some(&baseline), &current);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_only_languages_never_claim_a_graph_extractor() {
        for lang_id in TAG_ONLY_LANGUAGES {
            assert!(
                !lang::has_graph(lang_id),
                "{lang_id} is listed as tag-only but has_graph says otherwise — update \
                 TAG_ONLY_LANGUAGES or lang::from_path, whichever one just changed"
            );
        }
    }

    #[test]
    fn resolution_rate_of_a_language_with_no_graph_extractor_is_zero_not_nan() {
        // Mirrors ResolveStats::resolution_rate's own zero-call-sites contract: a language that
        // never produces a call site (because it never produces a graph node at all) must read as
        // 0.0, never NaN — NaN would poison any aggregate that touches it.
        let stats = ResolveStats::default();
        assert_eq!(stats.resolution_rate(), 0.0);
        assert!(!stats.resolution_rate().is_nan());
    }

    #[test]
    fn drift_classification_flags_a_node_count_drop_as_regressed_even_if_resolution_rate_improved()
    {
        let old = AppMetrics {
            files: FileMetrics {
                files_total: 1,
                files_chunked: 1,
                files_skipped_unsupported: 0,
                files_graphable: 1,
            },
            graph: GraphMetrics {
                nodes: 10,
                edges: 10,
                calls_edges: 2,
                route_nodes: 0,
                external_service_nodes: 0,
            },
            resolution: ResolutionMetrics {
                calls_resolved: 1,
                calls_ambiguous: 0,
                calls_unresolved: 1,
                resolution_rate: 0.5,
            },
            languages: BTreeMap::new(),
            file_coverage: Some(1.0),
            framework_coverage: 0,
        };
        let mut new = old.clone();
        new.graph.nodes = 9; // regressed
        new.resolution.calls_resolved = 2;
        new.resolution.resolution_rate = 1.0; // improved, but must not mask the node drop

        assert_eq!(classify_drift(&old, &new), Drift::Regressed);
    }

    #[test]
    fn drift_classification_flags_improvement_only_when_nothing_got_worse() {
        let old = AppMetrics {
            files: FileMetrics {
                files_total: 1,
                files_chunked: 1,
                files_skipped_unsupported: 0,
                files_graphable: 1,
            },
            graph: GraphMetrics {
                nodes: 10,
                edges: 10,
                calls_edges: 2,
                route_nodes: 0,
                external_service_nodes: 0,
            },
            resolution: ResolutionMetrics {
                calls_resolved: 1,
                calls_ambiguous: 0,
                calls_unresolved: 1,
                resolution_rate: 0.5,
            },
            languages: BTreeMap::new(),
            file_coverage: Some(1.0),
            framework_coverage: 0,
        };
        let mut new = old.clone();
        new.graph.nodes = 11;

        assert_eq!(classify_drift(&old, &new), Drift::Improved);
    }

    #[test]
    fn drift_classification_is_unchanged_when_all_three_signals_hold_steady() {
        let m = AppMetrics {
            files: FileMetrics {
                files_total: 1,
                files_chunked: 1,
                files_skipped_unsupported: 0,
                files_graphable: 1,
            },
            graph: GraphMetrics {
                nodes: 10,
                edges: 10,
                calls_edges: 2,
                route_nodes: 0,
                external_service_nodes: 0,
            },
            resolution: ResolutionMetrics {
                calls_resolved: 1,
                calls_ambiguous: 0,
                calls_unresolved: 1,
                resolution_rate: 0.5,
            },
            languages: BTreeMap::new(),
            file_coverage: Some(1.0),
            framework_coverage: 0,
        };
        assert_eq!(classify_drift(&m, &m.clone()), Drift::Unchanged);
    }

    #[test]
    fn drift_classification_tolerates_1ulp_resolution_rate_noise_from_a_json_round_trip() {
        // Reproduces the exact discrepancy this test suite hit for real: identical integer counters
        // (7 resolved / 4 ambiguous / 27 unresolved), but `serde_json::from_str` on the persisted
        // text produced a resolution_rate one ULP away from `ResolveStats::resolution_rate()`'s
        // freshly computed value. Without RESOLUTION_RATE_EPSILON this reports REGRESSED/IMPROVED for
        // an app where nothing actually changed.
        let old = AppMetrics {
            files: FileMetrics {
                files_total: 5,
                files_chunked: 5,
                files_skipped_unsupported: 0,
                files_graphable: 4,
            },
            graph: GraphMetrics {
                nodes: 24,
                edges: 27,
                calls_edges: 7,
                route_nodes: 0,
                external_service_nodes: 0,
            },
            resolution: ResolutionMetrics {
                calls_resolved: 7,
                calls_ambiguous: 4,
                calls_unresolved: 27,
                resolution_rate: 0.18421052631578944,
            },
            languages: BTreeMap::new(),
            file_coverage: Some(1.0),
            framework_coverage: 0,
        };
        let mut new = old.clone();
        new.resolution.resolution_rate = 0.18421052631578946; // 1 ULP higher, same integer counters

        assert_eq!(classify_drift(&old, &new), Drift::Unchanged);
    }

    #[test]
    fn drift_classification_still_flags_a_resolution_rate_drop_larger_than_the_epsilon() {
        let old = AppMetrics {
            files: FileMetrics {
                files_total: 5,
                files_chunked: 5,
                files_skipped_unsupported: 0,
                files_graphable: 4,
            },
            graph: GraphMetrics {
                nodes: 24,
                edges: 27,
                calls_edges: 7,
                route_nodes: 0,
                external_service_nodes: 0,
            },
            resolution: ResolutionMetrics {
                calls_resolved: 7,
                calls_ambiguous: 4,
                calls_unresolved: 27,
                resolution_rate: 0.5,
            },
            languages: BTreeMap::new(),
            file_coverage: Some(1.0),
            framework_coverage: 0,
        };
        let mut new = old.clone();
        new.resolution.resolution_rate = 0.4; // a real drop, nowhere near the 1e-9 tolerance

        assert_eq!(classify_drift(&old, &new), Drift::Regressed);
    }
}
