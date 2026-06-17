# Jyowo Backend Guidelines

This file is the entry point for Jyowo backend engineering rules.

The backend specification is intentionally compact. Do not split it into many narrow files unless a document becomes hard to review.

## Active Specification

- [Runtime](./backend-runtime.md)
- [Engineering](./backend-engineering.md)
- [Quality](./backend-quality.md)

## Reading Order

For backend work, read:

1. [Runtime](./backend-runtime.md)
2. [Engineering](./backend-engineering.md)
3. [Quality](./backend-quality.md)

## Normative Language

`MUST`, `SHOULD`, and `MAY` mark rule strength.

- `MUST`: required for this project.
- `SHOULD`: default choice unless there is a documented reason.
- `MAY`: allowed.

Rules are not enough by themselves. Each topic document also includes context, examples, and forbidden patterns.

## File Policy

Active backend docs should stay small in count:

```text
agent-harness-backend-development-guidelines.md
backend-runtime.md
backend-engineering.md
backend-quality.md
```
