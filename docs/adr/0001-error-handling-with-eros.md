# Error handling: eros typed unions on ports, opaque inside adapters

The restructured workspace (core / adapters / egui app) drops `thiserror` and uses `eros` for error composition. Core defines its own hand-rolled error types (`#[derive(Debug)]` + manual `Display` + `std::error::Error`), one enum per port or failure domain, whose variants carry what the model needs to *act* — never `String` payloads except an opaque `source: Box<dyn Error + Send + Sync>` that keeps the adapter's cause in the chain. Ports return a plain `Result<T, CoreErr>` when they have one failure domain and `eros::Result<T, (A, B)>` (a typed `ErrorUnion`) only where two error types the model handles differently genuinely combine; there is no catch-all `CoreError` enum. Inside adapters, `fetch_maps` and the app, eros is used freely — untyped `eros::Result<T>`, `bail!`, `ensure!` — and the `impl Port` method is where that opaque error is mapped into the port's declared core type; core itself never uses `bail!`/`ensure!`. Context is boundary-only: one `.context(...)` per port-impl method and at `main` startup, never `#[context]` and never inside core.

## Consequences

- eros features: core and adapters set `default-features = false`; the app and `fetch_maps` enable `context` + `backtrace`. Cargo unifies features workspace-wide, so this documents intent rather than isolating core — core must compile and test either way.
- Logging is not domain behaviour: there is no `Effect::Log`. Every port error is logged once at the app edge with `{err:?}` (full chain + backtrace via `source`); adapters do not log.
- Core's model decides which errors the user sees, turning them into Notifications (see `CONTEXT.md`) when they change what the user can see or do (map failed to load, screenshots folder unavailable, update failed, GPU cannot show maps). Transient/internal failures (unparsable screenshot filename, watcher noise, background update-check failure) stay log-only.
- Bundled-asset failures (`maps.ron` not parsing) are fatal: `main() -> eros::Result<()>` prints the error and exits non-zero rather than running with no maps. `expect` is reserved for invariants that cannot fail without a broken build.

## Considered options

- Keep `thiserror` for derives + eros for composition — rejected to hold one error vocabulary; core has few enough types that manual `Display` is cheap.
- `derive_more` — rejected for the same reason; no extra core dependency for a few `Display` impls.
- One `CoreError` enum — rejected; it recreates the catch-all enum eros exists to avoid and hides which port fails how.
