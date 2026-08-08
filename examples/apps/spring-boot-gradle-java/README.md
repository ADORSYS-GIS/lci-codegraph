# Spring Boot Stock Service

A Spring Boot 3 REST API for inventory, built with Gradle (Groovy DSL), demonstrating Spring annotation argument shapes and MongoDB repository extraction.

## What this sample demonstrates

- **Annotation argument variations**: routes use named arguments (`@RequestMapping(path = ...)`, `@GetMapping(value = ...)`), bare `@RequestMapping` with method enum, and array paths (`@GetMapping({"/admin/stock", "/admin/inventory"})`) that produce two route nodes from one handler.
- **Class-level and bare decorators**: `StockController` has class-level `@RequestMapping(path = "/api/stock")`; `AdminController` has no class-level prefix (just `@Controller`) and no path argument, so its routes are bare.
- **Non-JPA Spring Data**: `StockRepository extends MongoRepository<StockItem, String>` resolves the same way as JPA—marker-interface detection applies to any Spring Data dialect.
- **Method resolution through controller→service→repository**: the call chain is visible across all three layers.

## The graph

**40 nodes, 40 edges; 13 calls.** Six route nodes: `route:GET:/api/stock/{sku}`, `route:PUT:/api/stock/{sku}`, `route:PATCH:/api/stock/{sku}/reserve`, `route:HEAD:/api/stock/health`, `route:GET:/admin/stock`, `route:GET:/admin/inventory`. No external services. Each route calls its handler; handlers call `StockServiceImpl` methods; service methods call repository methods that are declared locally on `StockRepository`.

## What it does NOT show

Inherited `MongoRepository` methods (`save()`, `findById()`) are not extracted, for the same reason as JPA in the Maven sample—the declarations live in Spring Data's source, not here.

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
