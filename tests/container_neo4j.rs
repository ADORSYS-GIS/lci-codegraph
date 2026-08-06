//! Proves the crate's whole reason for existing: a host loads `Graph` into Neo4j and then serves
//! two retrieval queries against it. This suite writes the fixture graph into a *real* Neo4j
//! (via `testcontainers_modules::neo4j`) using the exact Cypher the control plane runs in
//! production (`services/control-plane/src/integrations/neo4j.rs::upsert_graph` /
//! `find_symbol` / `get_callers`, mirrored here verbatim since this crate cannot depend on the
//! control plane), then asserts the retrieval queries return the right answers for relationships
//! that are verifiable by eye in the fixture at `tests/fixtures/sample-repo`.
//!
//! Skips cleanly (prints and returns, does not fail) when no Docker daemon is reachable.
//!
//! Wall-clock: dominated by the Neo4j container's own startup (image pull if not cached, then a
//! JVM boot) — roughly 10-20s per test on a warm image cache, longer cold.

#[path = "support/mod.rs"]
mod support;

use std::path::PathBuf;

use lci_codegraph::{GraphEdge, GraphNode, WalkOptions, walk_checkout};
use neo4rs::{Graph as Neo4jGraph, query};
use testcontainers_modules::{neo4j::Neo4j, testcontainers::runners::AsyncRunner};

const REPO_ID: i64 = 777;
const COMMIT: &str = "container-neo4j-test-commit";

/// A symbol returned by a graph query — mirrors
/// `services/control-plane/src/integrations/neo4j.rs::SymbolHit`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SymbolHit {
    node_id: String,
    label: String,
    source_file: String,
    start_line: i64,
}

fn symbol_from_row(row: &neo4rs::Row) -> Option<SymbolHit> {
    Some(SymbolHit {
        node_id: row.get("node_id").ok()?,
        label: row.get("label").unwrap_or_default(),
        source_file: row.get("source_file").unwrap_or_default(),
        start_line: row.get("start_line").unwrap_or(0),
    })
}

/// Mirrors `upsert_graph` in the control plane's `neo4j.rs`: generic `:Symbol` nodes keyed by
/// `(repo_id, commit, node_id)`, generic `[:REL {relation}]` edges, MERGE so re-running is
/// idempotent. Kept byte-for-byte identical to the production Cypher so this suite actually proves
/// the downstream write, not a look-alike.
async fn upsert_graph(
    graph: &Neo4jGraph,
    repository_id: i64,
    commit_sha: &str,
    nodes: &[GraphNode],
    edges: &[GraphEdge],
) -> (usize, usize) {
    let mut txn = graph.start_txn().await.expect("begin neo4j txn");

    for n in nodes {
        txn.run(
            query(
                "MERGE (s:Symbol {repo_id: $repo, commit: $commit, node_id: $id}) \
                 SET s.label = $label, s.source_file = $file, s.start_line = $line",
            )
            .param("repo", repository_id)
            .param("commit", commit_sha)
            .param("id", n.node_id.as_str())
            .param("label", n.label.as_str())
            .param("file", n.source_file.as_str())
            .param("line", n.start_line),
        )
        .await
        .expect("merge symbol node");
    }

    for e in edges {
        txn.run(
            query(
                "MATCH (a:Symbol {repo_id: $repo, commit: $commit, node_id: $src}) \
                 MATCH (b:Symbol {repo_id: $repo, commit: $commit, node_id: $dst}) \
                 MERGE (a)-[r:REL {relation: $rel}]->(b)",
            )
            .param("repo", repository_id)
            .param("commit", commit_sha)
            .param("src", e.source.as_str())
            .param("dst", e.target.as_str())
            .param("rel", e.relation.as_str()),
        )
        .await
        .expect("merge edge");
    }

    txn.commit().await.expect("commit neo4j txn");
    (nodes.len(), edges.len())
}

/// Mirrors `find_symbol` in the control plane's `neo4j.rs`: case-insensitive substring match over
/// label / node_id / source_file, scoped to one repo snapshot.
async fn find_symbol(
    graph: &Neo4jGraph,
    repository_id: i64,
    commit_sha: &str,
    term: &str,
    limit: i64,
) -> Vec<SymbolHit> {
    let mut rows = graph
        .execute(
            query(
                "MATCH (s:Symbol {repo_id: $repo, commit: $commit}) \
                 WHERE toLower(s.label) CONTAINS toLower($term) \
                    OR toLower(s.node_id) CONTAINS toLower($term) \
                    OR toLower(s.source_file) CONTAINS toLower($term) \
                 RETURN s.node_id AS node_id, s.label AS label, s.source_file AS source_file, \
                        s.start_line AS start_line \
                 LIMIT $limit",
            )
            .param("repo", repository_id)
            .param("commit", commit_sha)
            .param("term", term)
            .param("limit", limit),
        )
        .await
        .expect("find_symbol query");

    let mut hits = Vec::new();
    while let Some(row) = rows.next().await.expect("find_symbol row") {
        if let Some(hit) = symbol_from_row(&row) {
            hits.push(hit);
        }
    }
    hits
}

/// Mirrors `get_callers` in the control plane's `neo4j.rs`: reverse `calls` traversal.
async fn get_callers(
    graph: &Neo4jGraph,
    repository_id: i64,
    commit_sha: &str,
    node_id: &str,
    limit: i64,
) -> Vec<SymbolHit> {
    let mut rows = graph
        .execute(
            query(
                "MATCH (caller:Symbol {repo_id: $repo, commit: $commit}) \
                       -[:REL {relation: 'calls'}]-> \
                       (target:Symbol {repo_id: $repo, commit: $commit, node_id: $id}) \
                 RETURN caller.node_id AS node_id, caller.label AS label, \
                        caller.source_file AS source_file, caller.start_line AS start_line \
                 LIMIT $limit",
            )
            .param("repo", repository_id)
            .param("commit", commit_sha)
            .param("id", node_id)
            .param("limit", limit),
        )
        .await
        .expect("get_callers query");

    let mut hits = Vec::new();
    while let Some(row) = rows.next().await.expect("get_callers row") {
        if let Some(hit) = symbol_from_row(&row) {
            hits.push(hit);
        }
    }
    hits
}

async fn node_count(graph: &Neo4jGraph, repository_id: i64, commit_sha: &str) -> i64 {
    let mut rows = graph
        .execute(
            query("MATCH (s:Symbol {repo_id: $repo, commit: $commit}) RETURN count(s) AS n")
                .param("repo", repository_id)
                .param("commit", commit_sha),
        )
        .await
        .expect("count nodes");
    rows.next()
        .await
        .expect("count row")
        .expect("count present")
        .get::<i64>("n")
        .unwrap()
}

async fn edge_count(graph: &Neo4jGraph, repository_id: i64, commit_sha: &str) -> i64 {
    let mut rows = graph
        .execute(
            query(
                "MATCH (:Symbol {repo_id: $repo, commit: $commit})-[r:REL]->\
                 (:Symbol {repo_id: $repo, commit: $commit}) RETURN count(r) AS n",
            )
            .param("repo", repository_id)
            .param("commit", commit_sha),
        )
        .await
        .expect("count edges");
    rows.next()
        .await
        .expect("count row")
        .expect("count present")
        .get::<i64>("n")
        .unwrap()
}

/// Walk the fixture with the graph on. This is the same fixture `tests/parity.rs` snapshots against
/// `tests/golden/sample-repo.graph.json`, so the shape of `add()`/`print_result()`/`log()` and the
/// `Circle`/`Square` methods below is verifiable by eye against that golden.
fn fixture_root() -> PathBuf {
    support::fixture_root()
}

async fn start_neo4j_or_skip(
    test_name: &str,
) -> Option<(
    testcontainers::ContainerAsync<testcontainers_modules::neo4j::Neo4jImage>,
    Neo4jGraph,
)> {
    if !support::docker_available() {
        support::print_skip_message(test_name);
        return None;
    }
    let container = Neo4j::default()
        .start()
        .await
        .expect("start neo4j testcontainer");
    let uri = format!(
        "bolt://{}:{}",
        container.get_host().await.expect("container host"),
        container
            .image()
            .bolt_port_ipv4()
            .expect("bolt port mapping"),
    );
    let user = container.image().user().expect("default neo4j user");
    let pass = container.image().password().expect("default neo4j pass");
    let graph = Neo4jGraph::new(&uri, user, pass)
        .await
        .expect("connect to neo4j testcontainer");
    Some((container, graph))
}

#[tokio::test]
async fn graph_round_trips_through_neo4j_and_serves_correct_retrieval_queries() {
    let Some((_container, graph)) =
        start_neo4j_or_skip("graph_round_trips_through_neo4j_and_serves_correct_retrieval_queries")
            .await
    else {
        return;
    };

    let out = walk_checkout(
        &fixture_root(),
        &WalkOptions::builder().build_graph(true).build(),
    )
    .expect("walk fixture");
    assert!(!out.graph.nodes.is_empty(), "fixture must produce nodes");
    assert!(!out.graph.edges.is_empty(), "fixture must produce edges");

    let (written_nodes, written_edges) =
        upsert_graph(&graph, REPO_ID, COMMIT, &out.graph.nodes, &out.graph.edges).await;
    assert_eq!(written_nodes, out.graph.nodes.len());
    assert_eq!(written_edges, out.graph.edges.len());

    // `graph_get_callers` on `add()` (src/math.rs:2) must return exactly `main()` (src/main.rs:3) —
    // per the committed golden (tests/golden/sample-repo.graph.json), `main()` is the only caller.
    let add_callers = get_callers(&graph, REPO_ID, COMMIT, "src/math.rs#2:add", 10).await;
    assert_eq!(
        add_callers.len(),
        1,
        "add() must have exactly one caller, got {add_callers:?}"
    );
    assert_eq!(add_callers[0].node_id, "src/main.rs#3:main");
    assert_eq!(add_callers[0].source_file, "src/main.rs");
    assert_eq!(add_callers[0].start_line, 3);

    // `log()` (src/math.rs:11) is called only by `print_result()` (src/math.rs:7).
    let log_callers = get_callers(&graph, REPO_ID, COMMIT, "src/math.rs#11:log", 10).await;
    assert_eq!(log_callers.len(), 1, "log_callers = {log_callers:?}");
    assert_eq!(log_callers[0].node_id, "src/math.rs#7:print_result");

    // `area()` (src/shapes.rs:13) is called only by `Circle`'s `describe()` (src/shapes.rs:35).
    let area_callers = get_callers(&graph, REPO_ID, COMMIT, "src/shapes.rs#13:area", 10).await;
    assert_eq!(area_callers.len(), 1, "area_callers = {area_callers:?}");
    assert_eq!(area_callers[0].node_id, "src/shapes.rs#35:describe");

    // Negative: `Square::new` (src/shapes.rs:25) is defined but never called anywhere in the
    // fixture (only `Circle::new` at src/shapes.rs:9 is called, from `make_unit_circle`) — it must
    // have zero callers, not merely "fewer".
    let square_new_callers = get_callers(&graph, REPO_ID, COMMIT, "src/shapes.rs#25:new", 10).await;
    assert!(
        square_new_callers.is_empty(),
        "Square::new is never called in the fixture; got {square_new_callers:?}"
    );

    // `graph_find_symbol` by partial label: "add" matches exactly `add()` and reports its real
    // location.
    let add_hits = find_symbol(&graph, REPO_ID, COMMIT, "add", 10).await;
    assert_eq!(add_hits.len(), 1, "add_hits = {add_hits:?}");
    assert_eq!(add_hits[0].node_id, "src/math.rs#2:add");
    assert_eq!(add_hits[0].label, "add()");
    assert_eq!(add_hits[0].source_file, "src/math.rs");
    assert_eq!(add_hits[0].start_line, 2);

    // `graph_find_symbol` by partial name matches BOTH constructors ("new") — proves the query
    // isn't accidentally over-scoped to a single hit.
    let new_hits = find_symbol(&graph, REPO_ID, COMMIT, "new", 10).await;
    let new_ids: std::collections::BTreeSet<_> =
        new_hits.iter().map(|h| h.node_id.as_str()).collect();
    assert_eq!(
        new_ids,
        std::collections::BTreeSet::from(["src/shapes.rs#9:new", "src/shapes.rs#25:new"]),
        "new_hits = {new_hits:?}"
    );

    // Scope isolation: the same query under a different repo id returns nothing, proving the
    // `repo_id` property actually partitions the graph (not just the commit).
    assert!(
        find_symbol(&graph, REPO_ID + 1, COMMIT, "add", 10)
            .await
            .is_empty(),
        "a different repo_id must not see this snapshot's symbols"
    );
}

#[tokio::test]
async fn rewriting_the_same_commit_is_idempotent_merge_not_insert() {
    let Some((_container, graph)) =
        start_neo4j_or_skip("rewriting_the_same_commit_is_idempotent_merge_not_insert").await
    else {
        return;
    };

    let out = walk_checkout(
        &fixture_root(),
        &WalkOptions::builder().build_graph(true).build(),
    )
    .expect("walk fixture");

    upsert_graph(&graph, REPO_ID, COMMIT, &out.graph.nodes, &out.graph.edges).await;
    let nodes_after_first = node_count(&graph, REPO_ID, COMMIT).await;
    let edges_after_first = edge_count(&graph, REPO_ID, COMMIT).await;
    assert_eq!(nodes_after_first, out.graph.nodes.len() as i64);
    assert_eq!(edges_after_first, out.graph.edges.len() as i64);

    // Re-run the exact same write. MERGE (not CREATE) means this must not duplicate anything — the
    // regression this suite exists to catch.
    upsert_graph(&graph, REPO_ID, COMMIT, &out.graph.nodes, &out.graph.edges).await;
    let nodes_after_second = node_count(&graph, REPO_ID, COMMIT).await;
    let edges_after_second = edge_count(&graph, REPO_ID, COMMIT).await;
    assert_eq!(
        nodes_after_second, nodes_after_first,
        "re-upserting the same commit must not duplicate nodes"
    );
    assert_eq!(
        edges_after_second, edges_after_first,
        "re-upserting the same commit must not duplicate edges"
    );

    // And the retrieval queries still see exactly one caller after the duplicate write.
    let add_callers = get_callers(&graph, REPO_ID, COMMIT, "src/math.rs#2:add", 10).await;
    assert_eq!(add_callers.len(), 1, "add_callers = {add_callers:?}");
}
