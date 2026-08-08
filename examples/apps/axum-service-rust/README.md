# Axum Ticket Service

An Axum HTTP service written in Rust, demonstrating route discovery limitations and call resolution through the native Rust extractor.

## What this sample demonstrates

- **Routes are runtime method calls, not annotations**: Axum registers routes via `.route("/api/tickets", get(handlers::list_tickets))` on a `Router` struct. These are function calls, not decorators, so there is no static equivalent to Spring's `@GetMapping` nodes. Route nodes: zero.
- **Handler and service calls are visible**: functions like `list_tickets()`, `get_ticket()`, and `create_ticket()` in `handlers.rs` call methods on `repo.rs` (`find_all()`, `find_by_id()`, `insert()`). These cross-file calls resolve via the native Rust tree-sitter grammar.
- **Internal call chains work**: `repo.rs` methods call other local methods; `main.rs` calls `router()`, which calls `AppState::new()`. All resolved.

## The graph

**24 nodes, 27 edges; 7 calls.** No route nodes. Handler methods call repository methods (`list_tickets` → `find_all`, `get_ticket` → `find_by_id`, `create_ticket` → `insert`). Internal `repo.rs` calls exist (`insert` → `next_id`). Main entry point calls the router.

## What it does NOT show

The URL paths registered in `router()` (e.g., `"/api/tickets"`) are string literals in method arguments, not AST nodes, so they do not appear in the graph. A reader must pair the route registrations in `main.rs` with the handler functions to understand the full API surface.

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
