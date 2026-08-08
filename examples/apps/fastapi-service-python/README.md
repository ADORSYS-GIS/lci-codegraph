# FastAPI Catalog Service

A FastAPI HTTP service written in Python, demonstrating extraction without route nodes and the visibility of service-to-repository calls.

## What this sample demonstrates

- **FastAPI decorators are not extracted as routes**: `@app.get("/products/{sku}")` decorators are not extracted as route nodes, for the same reason NestJS decorators are not—route extraction is Spring-specific. Route nodes: zero.
- **Service methods call repository methods**: `ProductService` methods (`get()`, `list_all()`, `create()`) call `ProductRepository` methods (`find_by_sku()`, `find_all()`, `insert()`). These cross-file calls are visible.
- **Framework-agnostic call resolution**: the tags-based extractor works across languages and frameworks; it finds all method calls in scope, regardless of whether they are in Flask, FastAPI, Django, or plain Python.

## The graph

**18 nodes, 21 edges; 7 calls.** No route nodes, no external services. Handlers call service methods; service methods call repository methods. Internal calls within `repository.py` also resolve.

## What it does NOT show

The HTTP paths and methods (`/products/{sku}`, `GET`, `POST`, etc.) are in the decorator arguments and string literals, not extracted. A reader must trace the decorators to understand the API surface.

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
