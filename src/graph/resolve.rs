//! The cross-file name resolver: turns every file's recorded call sites into `calls` edges and returns
//! the canonicalised whole-repo [`Graph`]. Resolution policy (precision-favouring, ADR-0086 R5):
//! same-file definitions win; otherwise the global table is consulted. In **both** tables a call
//! resolves only to a **single** matching definition — a bare name matching several same-named defs is
//! **dropped and counted, not fanned out** to every candidate ([`pick`]); a path qualifier (`A::new`)
//! is used solely as a tiebreaker to single out the right one when there is genuine ambiguity.

use std::collections::HashMap;

use super::{Callable, FileSymbols, Graph, GraphEdge, GraphNode};

/// Outcome of resolving one call against a candidate set.
enum Pick<'a> {
    /// Exactly one definition matched — emit the edge.
    One(&'a str),
    /// Several same-named defs and no qualifier singles one out — drop and count (never fan out).
    Ambiguous,
    /// Nothing matched here — try the next table (or count as unresolved).
    None,
}

/// Resolve a call against `candidates` (all defs sharing the callee's bare name). A single candidate
/// resolves unless a **type** qualifier positively contradicts it (a *module* qualifier — the
/// candidate has no type scope — still matches, so `math::add` resolves to a free `add`). Several
/// candidates are [`Pick::Ambiguous`] unless the qualifier singles exactly one out — we never emit an
/// edge to every same-named def (the same-file fan-out bug).
fn pick<'a>(candidates: Option<&[&'a Callable]>, qualifier: Option<&str>) -> Pick<'a> {
    let candidates = match candidates {
        Some(c) if !c.is_empty() => c,
        _ => return Pick::None,
    };
    if let [only] = candidates {
        if let (Some(q), Some(scope)) = (qualifier, only.scope.as_deref())
            && q != scope
        {
            return Pick::None;
        }
        return Pick::One(only.node_id.as_str());
    }
    // Genuine same-name ambiguity: a type qualifier is the only thing that can break it.
    if let Some(q) = qualifier {
        let mut narrowed = candidates.iter().filter(|c| c.scope.as_deref() == Some(q));
        if let Some(first) = narrowed.next()
            && narrowed.next().is_none()
        {
            return Pick::One(first.node_id.as_str());
        }
    }
    Pick::Ambiguous
}

/// Resolve every file's call sites into `calls` edges with cross-file resolution, and return the
/// canonicalised whole-repo [`Graph`].
#[must_use]
pub fn resolve(files: Vec<FileSymbols>) -> Graph {
    // Global callable table: name → all callables across files (for cross-file resolution).
    let mut global: HashMap<&str, Vec<&Callable>> = HashMap::new();
    for f in &files {
        for c in &f.callables {
            global.entry(c.name.as_str()).or_default().push(c);
        }
    }

    let mut nodes: Vec<GraphNode> = Vec::new();
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut ambiguous = 0usize;
    let mut unresolved = 0usize;

    for f in &files {
        nodes.extend(f.nodes.iter().cloned());
        edges.extend(f.contains.iter().cloned());

        // Per-file callable table for same-file-first resolution.
        let mut local: HashMap<&str, Vec<&Callable>> = HashMap::new();
        for c in &f.callables {
            local.entry(c.name.as_str()).or_default().push(c);
        }

        for call in &f.calls {
            let qualifier = call.qualifier.as_deref();
            let local_pick = pick(local.get(call.name.as_str()).map(Vec::as_slice), qualifier);
            // Same-file definitions win on a clean single hit. Otherwise — no local match, OR a local
            // set too ambiguous to attribute — consult the global table, where a qualifier may still
            // single out exactly one definition (e.g. locals `A::new`+`B::new` don't shadow a global
            // `C::new()` call). A local ambiguity is only *counted* ambiguous if the global fails too.
            let local_ambiguous = matches!(local_pick, Pick::Ambiguous);
            let target = match local_pick {
                Pick::One(id) => Some(id.to_string()),
                Pick::None | Pick::Ambiguous => {
                    match pick(global.get(call.name.as_str()).map(Vec::as_slice), qualifier) {
                        Pick::One(id) => Some(id.to_string()),
                        Pick::Ambiguous => {
                            ambiguous += 1;
                            None
                        }
                        Pick::None => {
                            if local_ambiguous {
                                ambiguous += 1;
                            } else {
                                unresolved += 1;
                            }
                            None
                        }
                    }
                }
            };
            if let Some(target) = target {
                edges.push(GraphEdge {
                    source: call.caller.clone(),
                    target,
                    relation: "calls".to_string(),
                });
            }
        }
    }

    // Canonicalise: sort + dedup so output is deterministic (stable goldens, stable submit).
    nodes.sort();
    nodes.dedup();
    edges.sort();
    edges.dedup();

    tracing::debug!(
        nodes = nodes.len(),
        edges = edges.len(),
        ambiguous_calls = ambiguous,
        unresolved_calls = unresolved,
        "codegraph: resolved structural graph"
    );

    Graph { nodes, edges }
}

#[cfg(test)]
mod tests {
    //! Unit tests over the resolver internals: [`pick`]'s tie-break policy in isolation (no parser,
    //! no tags — just hand-built [`Callable`] candidate sets), plus [`resolve`]'s canonicalisation
    //! (dedup + sort) exercised directly against hand-built [`FileSymbols`]. End-to-end
    //! parse-through-resolve behaviour lives in `graph::tests`; these tests isolate the resolver logic
    //! itself so a regression here points straight at [`pick`]/[`resolve`], not the whole pipeline.
    use super::*;
    use crate::graph::CallSite;

    #[test]
    fn pick_with_no_candidates_returns_none() {
        assert!(matches!(pick(None, None), Pick::None));
    }

    #[test]
    fn pick_with_empty_candidate_slice_returns_none() {
        let empty: Vec<&Callable> = Vec::new();
        assert!(matches!(pick(Some(&empty), None), Pick::None));
    }

    #[test]
    fn pick_single_candidate_with_no_qualifier_resolves() {
        let c = Callable {
            name: "f".to_string(),
            node_id: "id1".to_string(),
            scope: None,
        };
        let cands = [&c];
        assert!(matches!(pick(Some(&cands), None), Pick::One(id) if id == "id1"));
    }

    #[test]
    fn pick_single_candidate_with_matching_type_qualifier_resolves() {
        // `A::new()` against a single `new` scoped to `A` — the qualifier agrees, so it resolves.
        let c = Callable {
            name: "new".to_string(),
            node_id: "a.rs#1:new".to_string(),
            scope: Some("A".to_string()),
        };
        let cands = [&c];
        assert!(matches!(pick(Some(&cands), Some("A")), Pick::One(id) if id == "a.rs#1:new"));
    }

    #[test]
    fn pick_single_candidate_with_mismatched_type_qualifier_is_none() {
        // `B::new()` against a single `new` scoped to `A` — a *type* qualifier that positively
        // contradicts the only candidate must not resolve to it.
        let c = Callable {
            name: "new".to_string(),
            node_id: "a.rs#1:new".to_string(),
            scope: Some("A".to_string()),
        };
        let cands = [&c];
        assert!(matches!(pick(Some(&cands), Some("B")), Pick::None));
    }

    #[test]
    fn pick_single_candidate_with_module_qualifier_and_no_scope_still_resolves() {
        // `math::add()` against a free function `add` (no type scope) — a *module* qualifier doesn't
        // contradict a scopeless candidate, so it still resolves (the doc example on `pick`).
        let c = Callable {
            name: "add".to_string(),
            node_id: "math.rs#1:add".to_string(),
            scope: None,
        };
        let cands = [&c];
        assert!(matches!(pick(Some(&cands), Some("math")), Pick::One(id) if id == "math.rs#1:add"));
    }

    #[test]
    fn pick_multiple_candidates_with_no_qualifier_is_ambiguous() {
        let a = Callable {
            name: "new".to_string(),
            node_id: "a".to_string(),
            scope: Some("A".to_string()),
        };
        let b = Callable {
            name: "new".to_string(),
            node_id: "b".to_string(),
            scope: Some("B".to_string()),
        };
        let cands = [&a, &b];
        assert!(matches!(pick(Some(&cands), None), Pick::Ambiguous));
    }

    #[test]
    fn pick_multiple_candidates_qualifier_singles_out_exactly_one() {
        let a = Callable {
            name: "new".to_string(),
            node_id: "a".to_string(),
            scope: Some("A".to_string()),
        };
        let b = Callable {
            name: "new".to_string(),
            node_id: "b".to_string(),
            scope: Some("B".to_string()),
        };
        let cands = [&a, &b];
        assert!(matches!(pick(Some(&cands), Some("A")), Pick::One(id) if id == "a"));
    }

    #[test]
    fn pick_multiple_candidates_qualifier_matching_none_is_ambiguous() {
        let a = Callable {
            name: "new".to_string(),
            node_id: "a".to_string(),
            scope: Some("A".to_string()),
        };
        let b = Callable {
            name: "new".to_string(),
            node_id: "b".to_string(),
            scope: Some("B".to_string()),
        };
        let cands = [&a, &b];
        assert!(matches!(pick(Some(&cands), Some("C")), Pick::Ambiguous));
    }

    #[test]
    fn pick_multiple_candidates_qualifier_matching_more_than_one_is_still_ambiguous() {
        // Two `new`s both scoped to `A` (e.g. two impls somehow sharing a scope name) — the qualifier
        // narrows to more than one, so it must stay ambiguous, never guess the first.
        let a1 = Callable {
            name: "new".to_string(),
            node_id: "a1".to_string(),
            scope: Some("A".to_string()),
        };
        let a2 = Callable {
            name: "new".to_string(),
            node_id: "a2".to_string(),
            scope: Some("A".to_string()),
        };
        let cands = [&a1, &a2];
        assert!(matches!(pick(Some(&cands), Some("A")), Pick::Ambiguous));
    }

    #[test]
    fn resolve_dedups_identical_call_sites_into_a_single_edge() {
        // Two identical `CallSite`s attributed to the same caller (e.g. `helper(); helper();`) must
        // collapse to one `calls` edge, not two.
        let mut fs = FileSymbols::default();
        fs.nodes.push(GraphNode {
            node_id: "f.rs".to_string(),
            label: "f.rs".to_string(),
            source_file: "f.rs".to_string(),
            start_line: 1,
        });
        fs.callables.push(Callable {
            name: "helper".to_string(),
            node_id: "f.rs#1:helper".to_string(),
            scope: None,
        });
        for _ in 0..2 {
            fs.calls.push(CallSite {
                caller: "f.rs#2:caller".to_string(),
                name: "helper".to_string(),
                qualifier: None,
            });
        }
        let g = resolve(vec![fs]);
        let calls: Vec<_> = g.edges.iter().filter(|e| e.relation == "calls").collect();
        assert_eq!(
            calls.len(),
            1,
            "duplicate identical call sites must dedup to one edge; got {calls:?}"
        );
    }

    #[test]
    fn resolve_sorts_and_dedups_nodes_for_deterministic_output() {
        let n_b = GraphNode {
            node_id: "b.rs".to_string(),
            label: "b.rs".to_string(),
            source_file: "b.rs".to_string(),
            start_line: 1,
        };
        let n_a = GraphNode {
            node_id: "a.rs".to_string(),
            label: "a.rs".to_string(),
            source_file: "a.rs".to_string(),
            start_line: 1,
        };
        let mut fs = FileSymbols::default();
        fs.nodes.push(n_b.clone());
        fs.nodes.push(n_a.clone());
        fs.nodes.push(n_b.clone()); // duplicate, out of order
        let g = resolve(vec![fs]);
        assert_eq!(
            g.nodes,
            vec![n_a, n_b],
            "nodes must be sorted by node_id and deduped regardless of input order"
        );
    }

    #[test]
    fn resolve_unresolvable_callee_produces_no_edge() {
        // A call to a name with no matching def anywhere (local or global) is dropped silently — no
        // guessed edge, and it must not be conflated with the `Ambiguous` counting path.
        let mut fs = FileSymbols::default();
        fs.nodes.push(GraphNode {
            node_id: "f.rs".to_string(),
            label: "f.rs".to_string(),
            source_file: "f.rs".to_string(),
            start_line: 1,
        });
        fs.calls.push(CallSite {
            caller: "f.rs#1:caller".to_string(),
            name: "nowhere_defined".to_string(),
            qualifier: None,
        });
        let g = resolve(vec![fs]);
        assert!(
            !g.edges.iter().any(|e| e.relation == "calls"),
            "an unresolvable callee must not produce a calls edge; got {:?}",
            g.edges
        );
    }
}
