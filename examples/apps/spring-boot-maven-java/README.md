# Spring Boot Orders Service

A Spring Boot 3 REST API for orders, built with Maven, demonstrating call resolution through Spring's dependency-injection model and the limits of syntactic extraction.

## What this sample demonstrates

- **HTTP routes extracted from Spring annotations**: five `@GetMapping`, `@PostMapping`, and `@DeleteMapping` annotations resolve to distinct route nodes, each wired to its handler method.
- **Resolved method calls through interfaces**: controller methods call interface methods on `OrderService`; the extractor resolves these by matching the receiver's declared type (`OrderService`) to its sole implementation (`OrderServiceImpl`).
- **Spring Data derived queries**: the repository's `findByCustomerEmail()` method resolves, because its declaration is in the source. Inherited methods (`save()`, `deleteById()`) do not resolve—they live in JPA's source, outside this checkout.
- **External service discovery**: the `@FeignClient` decorator on `ShippingClient` becomes a `service:shipping-service` node.

## The graph

**47 nodes, 47 edges; 13 of them calls.** Five route nodes: `route:GET:/api/orders/{id}`, `route:GET:/api/orders`, `route:GET:/api/orders/by-customer/{email}`, `route:POST:/api/orders`, `route:DELETE:/api/orders/{id}`. One external service: `service:shipping-service`. The call chain flows: each route → its controller method → the corresponding `OrderServiceImpl` method → either `OrderRepository.findByCustomerEmail()` (a derived query, declared locally), or `service:shipping-service` (from the `@FeignClient` name), or `Order.getId()` (resolved through the local type of `Order saved = ...`).

## What it does NOT show

Calls to inherited JPA methods (`orderRepository.save()`, `orderRepository.deleteById()`) disappear—the extractor only sees declarations in this project. There is no `save()` node and no edge from `OrderServiceImpl.create()` to a `save()` method, because `JpaRepository` is a Maven dependency, not local source. A real code graph would need to either ship dependency stubs or accept this honest limitation of syntactic extraction.

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
