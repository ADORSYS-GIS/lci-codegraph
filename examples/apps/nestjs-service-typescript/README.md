# NestJS Notification Service

A NestJS HTTP service written in TypeScript, demonstrating why decorators similar to Spring's are NOT extracted as routes.

## What this sample demonstrates

- **NestJS decorators are not extracted as routes**: `@Controller('notifications')` and `@Get(':id')` are structurally similar to Spring's `@RestController` and `@GetMapping`. They are deliberately not extracted. Route nodes: zero.
- **Service and repository calls are visible**: `NotificationController` methods call `NotificationService` methods; service methods call `NotificationRepository` methods. These cross-file calls resolve via the tags-query extractor for TypeScript.
- **Why the difference**: Spring extraction is hardcoded into the framework pass because HTTP routing is a first-class concept in Spring and central to understanding an app. NestJS routing is equally central, but the philosophy here is to be framework-agnostic: tag-based extraction works for any framework that uses dependency injection, while route extraction is Spring-specific.

## The graph

**19 nodes, 20 edges; 6 calls.** No route nodes, no external services. Controller → service → repository call chain is fully visible.

## What it does NOT show

The HTTP methods and paths (`GET :id`, `POST`, `DELETE :id`) are in the decorator arguments, not extracted. A reader must pair the decorators with the tags-based calls to understand the API surface.

---

Regenerate with `UPDATE_GOLDEN=1 cargo test --test examples`.
