# Working in this repository

Rules for anyone — human or AI — changing `lci-codegraph`. Every one of these exists because it was
learned the expensive way; the parenthetical is the incident, kept so the rule can be argued with
rather than merely obeyed.

## The doctrine

**This crate's whole value is that its output can be trusted.** A graph that is silently wrong is
worse than a graph that is missing, because a reviewer acts on it. Almost every rule below is a
consequence of that one sentence.

---

## 1. Verify claims against real output. Never against a report.

Run the thing. Read what it printed. Compare it to what was asserted.

This applies to your own reasoning, to a test suite's green tick, and — especially — to any summary a
sub-agent or tool hands you. In the sessions that produced the Spring support and the examples
gallery, sub-agents reported: documentation "validated against `graph.json`" for files written into a
directory containing no `graph.json`; a repository interface redeclaring inherited `JpaRepository`
methods, present only to make a sample's graph look fuller; and a coverage matrix reporting `0.0%`
where the truth was "nothing was attempted". All three passed their own review and were caught by
running the code and diffing the output.

**A green suite is evidence about the tests that exist, not about the change you made.**

## 2. An unchanged golden after a load-bearing change is a reason to investigate

When a fix you believe is significant produces a zero-line golden diff, the likely explanation is
that no fixture exercises the path — not that the change is safe.

(The declared-type qualifier fix took most Java instance calls from *unresolvable* to *resolvable*
and moved not one byte of any committed golden, because every Java fixture happened to call through a
static-style qualifier. It was verified against a purpose-built fixture instead, and the gap the
goldens had is now covered.)

## 3. Regenerate goldens; never hand-edit them

```bash
UPDATE_GOLDEN=1 cargo test --test language_goldens   # per-language fixtures
UPDATE_GOLDEN=1 cargo test --test parity             # the Rust sample-repo golden
UPDATE_GOLDEN=1 cargo test --test examples           # examples/apps/*/graph.json
UPDATE_GOLDEN=1 cargo test --test examples_metrics   # examples/metrics.json + METRICS.md
```

A committed graph is a *fact produced by this crate*. Hand-editing one converts it into a claim, and
the suite that compares against it stops meaning anything. When a regeneration produces a diff,
read every changed edge and say in the PR why it changed.

## 4. Never report a zero where "not attempted" is the truth

`0%` says *we tried and failed*. `n/a` says *there was nothing to try*. They are different facts and
a reader will act differently on them.

Concretely: `file_coverage` is `null` — not `0.0` — when a language has no extractor, and a
resolution rate renders `n/a (no call sites)` — not `0.0%` — when nothing was recorded to resolve.
Kotlin is the case that makes this real: no grammar, so no call site is ever recorded and no file is
ever graphed. Reporting either as zero states a failure that did not happen.

(`ResolveStats::resolution_rate` still returns `0.0` for an empty run — correct, so aggregation
cannot produce NaN. The *renderer* is where that sentinel has to stop.)

## 5. Resolve when a fact is provable; stay silent otherwise. Never guess.

Several same-named candidates with no disambiguating qualifier are **dropped and counted**, never
fanned out and never resolved to an arbitrary one (ADR-0086 R5). This is a deliberate trade of recall
for never mis-attributing a call, and it holds one layer up too: framework-level ambiguity does not
get relaxed treatment just because an annotation is attached to it.

Do not weaken this to make a number look better. If a metric improves because the resolver started
guessing, the metric got worse.

## 6. Keep the two failure buckets separate

`calls_ambiguous` means candidates existed and we declined to choose — a resolver-quality signal.
`calls_unresolved` usually means a call into a dependency or a built-in — expected, not a defect.
Collapsing them into one "missed" number makes a dependency-heavy repo look broken while hiding the
case that actually indicates the resolver could do better.

## 7. `calls` edges and `calls_resolved` are different quantities

The framework pass contributes already-resolved `route: → handler` edges that never pass through the
resolution loop. A Spring app's edge count therefore runs ahead of its resolved-call-site count by
roughly its route count. Dividing by the wrong one yields a plausible, wrong rate.

## 8. Framework knowledge lives behind a crate boundary

Anything that knows a specific library's API surface — annotation names, marker interfaces — belongs
in `crates/lci-codegraph-spring`, never in `src/`. If you find yourself writing an annotation name
under `src/`, stop; the seam is wrong.

The reason is maintenance honesty (design doc §5.2): a tree-sitter grammar changes rarely and is
maintained by someone else, while a framework's annotation surface churns every release. The crate
boundary is what makes *"does the core know about Spring?"* answerable by reading a manifest instead
of trusting a comment.

`FrameworkFacts` is **additive-only** by construction: it can contribute nodes, edges and call-target
candidates, and has no way to express "drop this candidate" or "override that resolution". Keep it
that way — it is what bounds the blast radius of getting a framework wrong.

## 9. Hard cutovers. No dormant code.

When replacing something, remove the old path in the same change. No feature flag that ships a
capability turned off, no `Option` parameter preserving old behaviour, no sibling entrypoint. A gate
must be a genuine content check (does this file import Spring?) rather than a switch someone might
forget to flip.

If a change is risky, verify it harder — do not hide it behind a flag.

## 10. Workspace commands need `--workspace`

```bash
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Plain `cargo test` covers the root package only and **silently skips every crate under `crates/`**.
That gap once meant two crates' entire test suites never ran in CI while it reported green.

Publishing is likewise workspace-wide and ordered: `cargo publish --workspace`. A root-only publish
cannot resolve the unpublished path dependencies. The version lives once, in
`[workspace.package].version`, and CI asserts every package and internal dependency requirement
matches the tag.

## 11. tree-sitter queries fail open — verify against `node-types.json`

`child_by_field_name("modifiers")` returns `None` for every Java declaration, because `modifiers` is
an **untagged positional child**, not a named field. The API does not error; it returns an empty
answer, and an extractor built on it produces a confident, entirely empty result.

Before relying on a field name, check the grammar's own `node-types.json`. When a walk returns
nothing, suspect the query before the input.

## 12. Say what you did not do

A PR that lists what is out of scope, and the limits of what shipped, is worth more than one that
implies completeness. The repository's own documentation names its gaps — inherited repository
methods that cannot resolve, Kotlin's missing grammar, `@Profile` being undecidable. Match that.

If part of the work is blocked or deliberately skipped, finish everything else and say plainly what
was left and why. Do not quietly narrow the scope.

## Before opening a PR

- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace` — all green.
- Container suites (`--features container-tests`) need Docker. If you cannot run them, say so in the
  PR rather than implying they passed; their invariants (no duplicate `node_id`s, no dangling edges,
  `start_line >= 1`, byte-identical output across runs) are worth checking by hand in the meantime.
- Fill in the [PR template](.github/PULL_REQUEST_TEMPLATE.md) fully: a real source-of-truth link, an
  AI Usage Declaration, and verification evidence that is command output rather than assertion.
- **Leave the human-verification checkboxes unchecked.** They are the author's to check, not the
  tool's.
