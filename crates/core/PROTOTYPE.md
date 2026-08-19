# PROTOTYPE — core crate skeleton (wayfinder ticket #54)

Throwaway. Built 2026-08-19 to *feel* the decided shape (ADR-0001..0006, tickets
#44/#46/#47/#48) before the spec is assembled. Not production; discard or keep
as a reference for `/implement`.

Run: `cargo test -p tarkov-map-core` (16 tests, ~0.2 s; `--nocapture` prints
model-state dumps after the interesting scenarios).

## What is here

| Area | File | What it exercises |
|---|---|---|
| Domain | `domain/{spaces,map,position}.rs` | euclid typed spaces (`Game`/`Image`/`Screen`), `Map` with baked `Projection`, `MapCatalog::try_new` validator, `PlayerPosition` + `freshness(now)`, hand parser `parse_screenshot_name` (no regex) with the TarkovMonitor yaw kept verbatim |
| Model | `model/mod.rs` (+ `tests.rs`) | sync `Model::handle(msg, now) -> Vec<Effect>`; 9 of the 16 decided messages, all 5 effects; selection, Trail (cap 20), Map Suggestion (exactly-one + mute), settings dir change, notifications dedupe, stale-decode drop |
| Ports | `ports/mod.rs`, `ports/fakes.rs` | `PositionSource` as RPITIT `impl Stream + Send + 'static` (no async-trait), `ImageDecoder` sync, the `Ports` bundle trait with assoc types; `FakePositionSource` (scripted `stream::iter`), `FakeDecoder` (gate-able), `FakePorts` behind `cfg(any(test, feature = "fakes"))` |
| Runner | `runner.rs` | generic `Runner<P: Ports>`: `JoinSet` + `Handle`, `DecodeImage` via `spawn_blocking` with named-slot cancellation, `WatchScreenshots`/`StopWatching` forwarding stream items as `Msg` + `wake` |
| eros | `ports/fakes.rs::eros_boundary` (test) | an adapter-style fn using `bail!/ensure!/into_dyn_union` mapped into the port's hand-rolled error at the `impl Port` method |

## Verdict (agent's read — for the human to react to)

The shape is **sane and not over-engineered**: the whole thing is ~900 lines incl.
tests, no `dyn`, no `async-trait`, no mocks, model tests need no runtime, runner
tests are two `#[tokio::test]`s. Nothing decided had to be bent to compile.
What *did* surface is a handful of small decision tweaks:

1. **`Effect::DecodeImage` should carry the `MapImageKey`**, not just the `MapId`.
   Otherwise the runner must hold the catalogue to look the key up (the skeleton
   fakes it with `maps/<id>.bc7z`). The model has the catalogue; the effect should
   carry what the port needs: `DecodeImage { map: MapId, image: MapImageKey }`.
2. **Initial `Tracking` is misleading.** Between `Model::new` issuing
   `WatchScreenshots` and the source's `Started(kind)` arriving, the model sits
   in `Tracking::Off(NoDocumentsFolder)` (see the `dump` lines). Either add a
   `Tracking::Starting` variant or make `Waiting` not need a `SourceKind` yet.
3. **`Model::new` needs `now`** (for `image: Loading { since }`) — #47's signature
   omitted it.
4. **`WatchError::WatchFailed(String)`** (from #44/#48) contradicts ADR-0001's
   "no String payloads except an opaque `source`". Skeleton uses
   `WatchFailed(Box<dyn Error + Send + Sync>)`; consequence: `PositionEvent`,
   `Msg`, `Tracking` are not `Clone`/`PartialEq` (tests use `matches!`, fine).
   Pick one and write it down.
5. **eros in core is unused.** With the decided ports (one hand-rolled error per
   port, no `ErrorUnion` anywhere) core's only eros usage is the test-only
   boundary demo. Proposal: core has **no eros dependency** at all; ADR-0001's
   "core disables eros defaults" becomes "core does not depend on eros".
6. **eros context does not survive the port boundary as specified.** Verified in a
   scratch crate with `context`+`backtrace` on: `ErrorUnion<AnyError>` has no
   `into_dyn_error()` (trait bounds unsatisfied for `AnyError`), and
   `into_inner()` — the only route to a `Box<dyn Error>` — returns the bare inner
   error, **dropping the `.context(...)` chain and backtrace**. So ADR-0001's
   "one `.context` per port-impl method, then map into the port's opaque `source`"
   loses that context. Options: (a) drop `.context` at port impls — the port error
   already carries the structured facts (`key`, `dir`) as fields, and the app logs
   `{err:?}` of *that*; `.context` stays only at `main`; (b) adapters wrap the
   union in a tiny `struct Cause(ErrorUnion)` that impls `Error` with
   `Display = {0:?}`. (a) is simpler and consistent with "fields over strings".
7. **eros gotcha**: `bail!/ensure!(cond, "literal {x:?}")` does *not* format a bare
   literal (it is `StrError::Static`); you must use the `(fmt, args…)` form.
   Worth one line in ADR-0001 or the standards doc.
8. **`spawn_blocking` cannot be cancelled, and runtime drop waits for it.** The
   cancellation test hung until the fake released *every* blocked decode: a
   `Notify` gate stores at most one permit, so the fake's gate must be a
   `Semaphore` (`add_permits(n)`). Testing-strategy detail; and it makes
   ADR-0002's `rt.shutdown_timeout(2 s)` load-bearing, not cosmetic.
9. Cosmetic: `MapCatalog` is non-empty by construction, so `is_empty()` is a
   constant `false` (only there for clippy's `len_without_is_empty`); consider
   `count()` instead of `len()`.

## Not exercised

Viewport maths, updater messages, `SettingsStore`/`ReleaseFeed` ports,
`PersistSettings` coalescing in the runner, Overlay queries (`offered_overlays`,
`OverlayItem`), `ViewFrame`. None of these looked risky from the decisions; they
are volume, not shape.
