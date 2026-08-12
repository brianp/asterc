# Plan: Arity-1 Named Arguments

status: pending
rfc: protocols amendment 2026-08-12
issue: https://github.com/brianp/asterc/issues/52

The rule: one argument may go unnamed, two or more must be named, everywhere.
Functions, methods, and constructors all follow the same convention. The label
stays legal at arity 1. Rationale lives in the protocols RFC amendment; this
plan is just the execution.

## Tasks

### 1. Typechecker: accept arity-1 positional on ordinary calls

In `typecheck/src/check_call.rs`, when the callee resolves to a function,
lambda, or method with exactly one declared parameter and the call has exactly
one argument with a synthesized name (`_0`), map it to the parameter. Arity 2+
positional keeps the current rejection with the "add `name:` before this" hint.

### 2. Constructor drift revert

Constructors currently map positional args to fields by index at any arity
(`check_call.rs`, constructor path around line 870). Tighten to the same
arity-1 rule: one field may be positional, two or more require names. The
diagnostic should hint the first missing field name.

### 3. Delete the subsumed carve-outs

- The `to_int` positional normalization special case (`check_call.rs:279` area)
  falls out of the general rule; remove the special-case code.
- Confirm `log`, `len`, `say`, and the other single-arg builtins dispatch
  through the same path instead of bespoke handling where possible.

### 4. Tests

- `tests/integration/named_args.rs`: new cases for arity-1 positional accepted
  (function, method, constructor), labeled arity-1 still accepted, arity-2
  positional rejected (function AND constructor), mixed positional+named
  rejected.
- Update existing tests that use multi-arg positional construction
  (e.g. `tests/integration/redundant_type_lint.rs:104`) to named form.
- `cargo build -p codegen && cargo test --workspace` green.

### 5. Docs

- `docs/src/content/docs/language/functions.mdx`: the named-argument rule
  section gets the arity-1 refinement.
- `internals/type-checker.mdx` and `internals/parser.mdx`: the
  positional-argument policy paragraphs.
- `reference/status.mdx`: Language table row for the rule; remove the roadmap
  bullet once implemented.

### 6. Book continuity

`workingfiles/books/the-narrators-guide-to-aster/editorial-notes.md` item 2 is
resolved by this rule: `say("I can write code")` becomes legal as written.
Chapter drafts should use that form.

## Non-goals

- No redundant-label lint at arity 1 (decided: allow both silently; a W-series
  lint can be a separate proposal if the redundancy ever bothers anyone).
- No paren-less calls. `say hello` stays fiction.
