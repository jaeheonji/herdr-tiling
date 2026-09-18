# Rust Coding Guidelines

These rules apply to all Rust code in this repository. Prefer the existing project
conventions when they are stricter. Optimize for correctness, clarity, and maintainability;
optimize performance only with evidence.

## Required checks

Before completing a change, agents MUST run every applicable command and report any command
that could not be run:

```sh
cargo fmt --all -- --check
cargo check --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo doc --no-deps --all-features
```

- **MUST** keep the repository warning-free and formatted by the repository's pinned
  `rustfmt`/toolchain configuration.
- **NEVER** disable a lint globally or weaken CI merely to make a change pass. Use the
  narrowest `#[allow(...)]` only with a comment explaining why.

## Code style and formatting

- **MUST** use `rustfmt` defaults: four-space indentation, a 100-column target, block
  indentation, and trailing commas in multiline constructs.
- **MUST** follow Rust naming conventions: `snake_case` for functions/modules, `UpperCamelCase`
  for types/traits, and `SCREAMING_SNAKE_CASE` for constants/statics.
- **MUST** choose names that express domain intent and units (`timeout_ms`, `PriceCents`).
- **NEVER** hand-format code against `rustfmt`, leave trailing whitespace, or create unrelated
  formatting churn.
- **NEVER** use cryptic abbreviations or encode type information in names.

## Documentation and comments

- **MUST** document every public item unless its meaning is genuinely self-evident under the
  project's lint policy.
- **MUST** describe contracts and rationale: invariants, units, ownership, side effects, and
  non-obvious trade-offs. Comments explain *why*, not a restatement of the code.
- **MUST** add `# Errors`, `# Panics`, and `# Safety` sections when applicable, plus runnable
  examples for non-trivial public APIs.
- **MUST** use `///` for item docs and `//!` for crate/module docs; keep documentation examples
  passing as doctests.
- **NEVER** leave stale comments, commented-out code, changelog notes, or TODOs without an
  issue/owner and actionable context.
- **NEVER** claim safety, complexity, or allocation behavior that the implementation does not
  guarantee.

## Type system

- **MUST** make invalid states hard to represent with newtypes, enums, and validated
  constructors.
- **MUST** use `Option<T>` for absence and `Result<T, E>` for recoverable failure.
- **MUST** prefer standard conversion traits (`From`, `TryFrom`, `AsRef`) and iterator traits
  over ad-hoc conversion or collection APIs.
- **MUST** add useful standard traits when semantics permit, especially `Debug`, `Clone`,
  `Eq`, `Hash`, `Default`, `Display`, `Send`, and `Sync`.
- **NEVER** use primitive obsession for domain values, boolean parameters with unclear meaning,
  or sentinel values such as `-1`/empty strings for absence.
- **NEVER** add generic parameters, trait objects, or lifetimes that do not provide a concrete
  API or implementation benefit.

```rust
struct Timeout(std::time::Duration);

enum CachePolicy {
    Bypass,
    Use { ttl: std::time::Duration },
}
```

## Error handling

- **MUST** distinguish recoverable errors from violated invariants: return `Result` for the
  former and reserve panics for programmer bugs or impossible states.
- **MUST** preserve the source error and add useful operation/context at abstraction boundaries.
- **MUST** expose structured, meaningful error types from reusable libraries; application
  boundaries may convert them into user-facing reports.
- **MUST** handle every `Result` intentionally and propagate with `?` when no local recovery is
  possible.
- **NEVER** use `unwrap()`/`expect()` on external input, I/O, synchronization, parsing, or any
  recoverable production path.
- **NEVER** silently discard errors with `let _ =`, broad fallback defaults, or logging followed
  by false success.
- **NEVER** ship reachable `todo!()`, `unimplemented!()`, or placeholder panics.

When an invariant is locally proven, prefer an explanatory `expect`:

```rust
let header = headers
    .get("content-type")
    .expect("content-type was inserted immediately above");
```

## Function design

- **MUST** give each function one clear responsibility and keep control flow shallow with early
  returns and `?`.
- **MUST** take the least restrictive useful input (`&str`, `&[T]`, `impl Iterator`, `AsRef<Path>`)
  and return owned data only when ownership transfer is required.
- **MUST** use methods when there is a natural receiver and builders for complex, optional
  construction.
- **MUST** validate inputs at the boundary where invalid data first becomes trusted.
- **NEVER** use output parameters, unexplained boolean flags, or functions whose behavior depends
  on hidden global state.
- **NEVER** split readable cohesive logic solely to satisfy an arbitrary line count.

## Struct and enum design

- **MUST** keep public struct fields private unless they are deliberately part of a stable data
  contract; enforce invariants through constructors and methods.
- **MUST** use enums for closed alternatives and include associated data in the relevant variant.
- **MUST** consider `#[non_exhaustive]` for public structs/enums expected to evolve.
- **MUST** order fields and variants consistently and derive only traits with correct semantics.
- **NEVER** implement `Deref` merely for method reuse; reserve it for smart-pointer behavior.
- **NEVER** add catch-all enum variants that erase useful state or error information.

## Testing

- **MUST** test observable behavior, boundary values, failure paths, and every bug fix with a
  regression test.
- **MUST** keep unit tests near private logic and integration tests under `tests/` for public
  behavior. Use doctests for public examples.
- **MUST** make tests deterministic, isolated, readable, and explicit about arrange/act/assert.
- **MUST** use property or fuzz testing for parsers, serializers, and invariant-heavy logic when
  the risk justifies it.
- **NEVER** depend on wall-clock sleeps, test order, network services, ambient machine state, or
  shared mutable global state.
- **NEVER** change or remove a valid test only to accommodate an implementation regression.
- **NEVER** chase coverage numbers with assertions that do not verify behavior.

## Imports and dependencies

- **MUST** group imports according to `rustfmt`, import explicit names, and keep visibility as
  narrow as possible.
- **MUST** justify every new dependency, minimize enabled features, and check maintenance,
  license, security, and MSRV compatibility.
- **MUST** commit `Cargo.lock` for binaries/applications; follow the repository policy for
  libraries.
- **NEVER** use wildcard imports outside intentionally scoped preludes or tests.
- **NEVER** add a dependency for functionality that is trivial, security-sensitive without
  review, or already provided by the standard library/current dependency graph.
- **NEVER** expose an unstable dependency's type in a stable public API without deliberate review.

## Rust best practices and safety

- **MUST** prefer ownership, borrowing, iterators, pattern matching, RAII, and standard traits to
  manual state or resource management.
- **MUST** keep `unsafe` blocks minimal and place a `// SAFETY:` comment immediately above each
  block explaining all required invariants and why they hold.
- **MUST** wrap unsafe internals in a small safe API and test the boundary; use Miri/sanitizers
  when the affected code can be exercised by them.
- **MUST** preserve backward compatibility of public APIs unless a breaking change is explicitly
  requested and documented.
- **NEVER** use `unsafe` to bypass the borrow checker without a demonstrated need and a reviewed
  safety argument.
- **NEVER** hold a blocking or async lock guard across `.await`, call user code while holding a
  lock, or perform blocking work on an async executor thread.
- **NEVER** rely on unspecified layout or representation; use `#[repr(...)]` only for a documented
  ABI/layout requirement.

## Memory and performance

- **MUST** establish a baseline and measure before and after non-trivial optimization.
- **MUST** consider algorithmic complexity first; document surprising complexity, allocations,
  and blocking behavior in public APIs.
- **MUST** reuse buffers and stream/iterate over data when it materially reduces peak memory or
  latency without obscuring correctness.
- **NEVER** clone, collect, allocate, box, or use `Arc<Mutex<_>>` reflexively; make ownership and
  synchronization costs intentional.
- **NEVER** sacrifice correctness or clear code for speculative micro-optimizations.
- **NEVER** assume release performance from debug builds or make performance claims without a
  reproducible benchmark/profile.

## Change discipline

- **MUST** keep changes focused, preserve unrelated user work, and update docs/tests when behavior
  or public APIs change.
- **MUST** inspect existing modules and conventions before introducing a new abstraction.
- **MUST** report assumptions, trade-offs, and unverified checks in the final handoff.
- **NEVER** introduce broad refactors, public API breaks, generated artifacts, or dependency
  upgrades unless required by the task.
- **NEVER** commit secrets, credentials, local configuration, build output, or debug logging with
  sensitive data.

## References

- [The Rust Style Guide](https://doc.rust-lang.org/style-guide/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/checklist.html)
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/)
- [The Rust Programming Language: Error Handling](https://doc.rust-lang.org/book/ch09-00-error-handling.html)
