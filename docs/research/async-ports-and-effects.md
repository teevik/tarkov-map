# Async ports and effects in a tokio-based hexagonal core

Research for wayfinder ticket #40 (parent map issue #35). Question: with tokio
allowed in core, how should driven ports and effect execution be expressed when
the core is async — native `async fn` in traits / RPITIT and their `dyn`
limits, `async-trait`, `trait-variant`, `dynosaur`, generics-over-dyn, and
channel/task designs — and how does an eframe/egui immediate-mode UI (sync
`logic()`/`ui()` per frame) integrate with a tokio runtime cleanly? Recommend a
pattern that keeps the core testable with hand-rolled fakes and does not leak
threading into the model.

## Sources

All findings are from primary sources, read on 2026-08-17. Local crate sources
are the versions currently in `Cargo.lock` (`~/.cargo/registry/src/index.crates.io-*/`).

- Rust language
  - "Announcing `async fn` and return-position `impl Trait` in traits" (Rust
    1.75, 2023-12-21) — <https://blog.rust-lang.org/2023/12/21/async-fn-rpit-in-traits/>
  - Reference, dyn compatibility rules —
    <https://doc.rust-lang.org/reference/items/traits.html#r-items.traits.dyn-compatible.associated-functions>
  - `async_fn_in_trait` lint — <https://doc.rust-lang.org/rustc/lints/listing/warn-by-default.html>
  - RFC 3654 Return Type Notation — <https://rust-lang.github.io/rfcs/3654-return-type-notation.html>,
    tracking <https://github.com/rust-lang/rust/issues/109417>
  - Rust 1.85 (async closures) — <https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/>
  - Project goals: 2024h2 async — <https://rust-lang.github.io/rust-project-goals/2024h2/async.html>;
    2026 "Native async fn dynamic dispatch in traits" — <https://rust-lang.github.io/goals/2026/afidt-box.html>;
    July 2025 update — <https://blog.rust-lang.org/2025/08/05/july-project-goals-update/>
  - Book ch. 18.2 (trait objects vs generics) — <https://doc.rust-lang.org/book/ch18-02-trait-objects.html>
- Crates
  - `async-trait` 0.1.92 (2026-08-08) — <https://github.com/dtolnay/async-trait>
  - `trait-variant` 0.1.3 (2026-07-22) — <https://github.com/rust-lang/impl-trait-utils>,
    <https://docs.rs/trait-variant/latest/trait_variant/attr.make.html>
  - `dynosaur` 0.3.1 (2026-07-03) — <https://github.com/spastorino/dynosaur>, <https://docs.rs/dynosaur>
  - tokio 1.53.1 — <https://docs.rs/tokio/1.53.1/tokio/> (`sync/mpsc`, `sync/watch`,
    `sync/oneshot`, `task/join_set.rs`, `task/blocking.rs`, `runtime/{runtime,handle}.rs`,
    `macros/select.rs`; local `tokio-1.53.1/src/`)
  - tokio-util 0.7.19 — <https://docs.rs/tokio-util/0.7.19/tokio_util/sync/struct.CancellationToken.html>
  - reqwest 0.13.1 `blocking` module docs (local `reqwest-0.13.1/src/blocking/mod.rs`, `client.rs`)
  - self_update 0.44.0 (local `self_update-0.44.0/Cargo.toml`, `src/http_client/reqwest.rs`)
- egui / eframe
  - egui 0.36.1 `Context` docs (local `egui-0.36.1/src/context.rs` L671-680, L1805-1869, L1953-1960, L4265-4269)
  - eframe 0.36.1 `App` trait (local `eframe-0.36.1/src/epi.rs` L150-228), `run_native` (`src/lib.rs` L240-300)
  - eframe CHANGELOG — <https://github.com/emilk/egui/blob/main/crates/eframe/CHANGELOG.md>
  - egui README FAQ "How do I use egui with `async`?" — <https://github.com/emilk/egui/blob/main/README.md>
  - egui repo: `crates/egui_demo_app/src/apps/http_app.rs`, `examples/hello_world_par`,
    `examples/external_eventloop_async`
  - Discussions — <https://github.com/emilk/egui/discussions/521>, <https://github.com/emilk/egui/discussions/2010>
  - `egui_inbox` 0.13.0 (2026-08-06) — <https://github.com/lucasmerlin/hello_egui/tree/main/crates/egui_inbox>
  - `poll-promise` 0.3.0 (2023-08-27, last commit 2023-09-29) — <https://github.com/EmbarkStudios/poll-promise>
  - `ehttp` 0.7.1 (2026-03-23) — <https://github.com/emilk/ehttp>
- This repo (current concurrency): `src/bin/tarkov-map/main.rs` (`request_image`,
  `poll_all_assets`), `assets.rs` (`AssetCache`), `screenshot_watcher.rs`,
  `updater.rs`, `demo.rs`, `src/bin/fetch_maps.rs`.

## TL;DR

- **Keep the model synchronous.** `Model::handle(&mut self, Msg) -> Vec<Effect>`
  is a pure state machine; nothing in it awaits, spawns, or knows about tokio.
  Cheap reads (clock, map catalogue) come in as **sync ports** (`&dyn Clock`,
  trivially dyn-compatible). Anything slow or side-effecting comes *out* as an
  **`Effect` value** and comes back *in* as a `Msg`. This is what the app already
  does informally: three separate `std::mpsc` + `thread::spawn` +
  `ctx.request_repaint()` loops, polled with `try_recv` once per frame.
- **Async lives in one place: an `Executor` (effect runner) that owns the tokio
  runtime handle, a `JoinSet`, cancellation tokens, and the `Msg` sender.** It is
  the only code that awaits. It talks to the outside through the **async driven
  ports** (`ImageDecoder`, `ReleaseFeed`, `PositionSource`, …).
- **Async port trait shape: native `async fn`/RPITIT + generics, no `dyn`.**
  Native async fn in traits is stable since 1.75 but "not object-safe" (no
  `dyn`); RTN is still unstable and native `dyn` async is a 2026 project goal,
  not shipped. Since the executor is generic (`Executor<P: Ports>`) and there is
  exactly one production adapter set plus one fake set, static dispatch is
  enough; write `fn f(&self) -> impl Future<Output = T> + Send` (or
  `#[trait_variant::make(Send)]`) so tasks can be spawned on the multi-thread
  runtime. Reach for `dynosaur` (rust-lang-endorsed interim, 0.3.1) only if a
  `dyn` port ever becomes necessary; `async-trait` (still maintained, 0.1.92) is
  the boring fallback. Neither is needed today.
- **Runtime ownership: the app crate builds one `tokio::runtime::Runtime`
  (multi-thread, 2 workers) in `main`, keeps the `EnterGuard`, hands a `Handle`
  to the executor, runs `eframe::run_native` on the main thread, and drops the
  runtime after it returns.** This is the pattern egui maintainers point to
  (discussion #521) and matches how `fetch_maps` already uses tokio.
- **Delivering results: `tokio::sync::mpsc::unbounded_channel::<Msg>()`** —
  `UnboundedSender::send` is sync ("can be used in both synchronous and
  asynchronous code"), the UI drains `try_recv` in `App::logic`, and the
  executor calls a `wake` callback (`ctx.request_repaint()`) after each send.
  `Context` is `Send + Sync`, cheap to clone, and "If called from outside the UI
  thread, the UI thread will wake up". egui's own FAQ recommends exactly
  "channels … make sure to use `try_recv`". `watch` is the right tool for the
  one "latest value wins" stream (player position).
- **CPU-bound work (zstd + BC7 decode) goes through `spawn_blocking`** (or rayon
  + oneshot); note that `spawn_blocking` tasks **cannot be aborted once
  running**, so cancellation on map switch is: abort the awaiting task via
  `AbortHandle`/`CancellationToken`, *and* have the model drop stale results by
  tagging every request with the map it was for (today's "non-active results are
  dropped" rule, made explicit).
- **Testing:** the model is tested with plain `#[test]` and no runtime at all
  (feed `Msg`s, assert state and returned `Effect`s). The executor is tested with
  `#[tokio::test]` (current-thread, `start_paused` for timers) against
  hand-rolled fake ports — trivial with native `async fn` on a generic.

## (a) Native `async fn` in traits, RPITIT, and `dyn`

- Stable since Rust 1.75 (2023-12-21). The announcement is explicit about the
  two caveats. Dyn: "Traits that use `-> impl Trait` and `async fn` are not
  object-safe, which means they lack support for dynamic dispatch." Send: "you
  as a trait author need to make a choice: Do you want your trait to work with
  multithreaded, work-stealing executors?" — bare `async fn` in a *public* trait
  warns (`async_fn_in_trait`, warn-by-default: "use of `async fn` in public
  traits is discouraged as auto trait bounds cannot be specified"), and the
  recommended desugaring is `fn fetch(&self, url: Url) -> impl Future<Output =
  HtmlBody> + Send;`, which "still allows the use of `async fn` within impls of
  the trait". The lint text also notes you may suppress it "if you plan to use
  the trait only in your own code" — a `pub(crate)` port does not trigger it.
- Reference dyn-compatibility rule: dispatchable functions must "Not have an
  opaque return type; that is, Not be an `async fn` (which has a hidden `Future`
  type). Not have a return position `impl Trait` type". A method with `where
  Self: Sized` is allowed on a dyn-compatible trait but not callable through
  `dyn`.
- Return Type Notation (RFC 3654, `T: Trait<method(..): Send>`) is **not
  stable** (tracking issue still open, feature-gated; no release-note entry
  through 1.97.1, current stable as of 2026-07-16). Async closures (`AsyncFn*`)
  are stable since 1.85.
- Native `dyn` async: the 2024h2 goal said "Async fn in traits do not currently
  support native dynamic dispatch … not currently prioritizing"; the 2026 goal
  "Native async fn dynamic dispatch in traits" proposes a nightly-only
  `dyn_box!()` call-site boxing macro and says "The dynosaur crate is a good
  workaround, but native language support would avoid proc macro complexity".
  Nothing has shipped on stable.

Practical reading: with native `async fn` you get zero-cost static dispatch and
`Send`-ness inferred from the impl (or pinned by the desugared bound), at the
price of no `Box<dyn Port>`. That is fine if the thing holding the port is
generic (next section) — and generic-plus-fake is exactly what a hand-rolled
fake test wants.

## (b) `async-trait`, `trait-variant`, `dynosaur`

| Crate | Version / date | What it does | When it earns its keep |
| --- | --- | --- | --- |
| `async-trait` | 0.1.92, 2026-08-08 (actively maintained) | Rewrites each `async fn` to return `Pin<Box<dyn Future + Send + 'async_trait>>` — dyn-compatible, always heap-boxes; `#[async_trait(?Send)]` drops `Send`. README: "The stabilization of async functions in traits in Rust 1.75 did not include support for using traits containing async functions as `dyn Trait`." | You need `Box<dyn Port>` / `&dyn Port` and do not mind boxing every call. Boring, universally understood. |
| `trait-variant` | 0.1.3, 2026-07-22 (rust-lang org) | `#[trait_variant::make(IntFactory: Send)] trait LocalIntFactory { async fn … }` generates `trait IntFactory: Send { fn make(&self) -> impl Future<Output = i32> + Send; }` plus a blanket impl so a `Send` variant implements the local one. Says nothing about `dyn` — does not provide it. | Library-grade traits that must offer both `Send` and `?Send` variants. For a private core it is equivalent to writing the desugared `impl Future + Send` by hand. |
| `dynosaur` | 0.3.1, 2026-07-03, still 0.x (MSRV 1.84 for 0.3.1) | `#[dynosaur::dynosaur(DynPort = dyn(box) Port)]` generates a `DynPort` struct "that implements `MyTrait` by delegating to the actual impls on the concrete type and wrapping the result in a box"; static dispatch stays unboxed; `dyn(box, ?Send)` form. Endorsed by the project-goals updates as the interim for AFIT + dyn ("last remaining open design question before releasing dynosaur 0.3 as a candidate for 1.0"). | You want native `async fn` in the trait *and* one place where a `dyn` is unavoidable (e.g. runtime-selected adapter). Boxing only on the dyn path. |

## (c) Generics over `dyn`

Book ch. 18.2: "If you'll only ever have homogeneous collections, using generics
and trait bounds is preferable because the definitions will be monomorphized at
compile time"; `dyn` "lookup incurs a runtime cost … also prevents the compiler
from choosing to inline". Applied here:

- The executor holds one set of driven ports for the process lifetime. Making it
  `Executor<P: Ports>` (or `Executor<D: ImageDecoder, R: ReleaseFeed, …>`) costs
  nothing at runtime, permits native `async fn`, and a test fake is just another
  `impl ImageDecoder for FakeDecoder`. There is no heterogeneous collection of
  ports anywhere in this app.
- The trade-off is compile-time monomorphization (two instantiations: real and
  fake) and slightly longer signatures. If the generic list grows past three or
  four, group them into one `trait Ports { type Decoder: ImageDecoder; … }` or a
  plain struct of generics.
- Sync ports (Clock, MapCatalog) can stay `&dyn Trait` if that reads better —
  sync traits are dyn-compatible and there is no boxing of futures involved.

## (d) Channel/task building blocks (tokio 1.53.1, tokio-util 0.7.19)

Facts that shape the design (quotes from the docs.rs / local sources listed above):

- **`mpsc` unbounded** — `UnboundedSender::send` "is not marked as `async`
  because sending a message to an unbounded channel never requires any form of
  waiting … can be used in both synchronous and asynchronous code without
  issues." Module guidance: "for sending a message _from sync to async_, you
  should use an unbounded Tokio `mpsc` channel" and "any channel method that
  isn't marked async can be called anywhere, including outside of the runtime."
  `Receiver::try_recv` is sync. `blocking_send`/`blocking_recv` **panic** "if
  called within an asynchronous execution context" — never use them from tasks.
- **`watch`** — "only retains the *last* sent value … useful for watching for
  changes to a value"; `borrow_and_update()` marks seen, `has_changed()` is a
  sync query; `Sender::send` is sync. Fits "latest player position".
- **`oneshot`** — "Since the `send` method is not async, it can be used
  anywhere … including from non-async code"; `Receiver::try_recv` "is useful to
  call from outside the context of an asynchronous task". Fits one
  request/one reply if a `poll-promise`-style handle is ever wanted.
- **`JoinSet`** — "When the `JoinSet` is dropped, all tasks in the `JoinSet` are
  immediately aborted"; `spawn` returns an `AbortHandle`; `abort_all`,
  `shutdown` (abort + drain). Task cancellation "is signalled … next time it
  yields at an `.await` point"; `spawn_blocking` tasks "cannot be aborted
  because they are not async … The exception is if the task has not started
  running yet."
- **`CancellationToken`** — `child_token()` ("cancelling a child token does not
  cancel the parent"), `cancel()` is sync, `cancelled()` is cancel-safe,
  `run_until_cancelled(fut) -> Option<T>` drops the future on cancel,
  `drop_guard()` cancels on drop unless disarmed.
- **Runtime vs Handle** — `tokio::spawn` "Panics if called from **outside** of
  the Tokio runtime"; `Runtime::enter()` returns an `EnterGuard` that makes
  `spawn`/`Handle::current()` work on the holding thread; `Handle::spawn` works
  from any thread with a moved `Handle`; `Handle::try_current()` never panics.
  `block_on` panics "if called within an asynchronous execution context".
  Dropping a `Runtime` "blocks until all spawned work has been stopped …
  `Drop` … waits forever"; `shutdown_timeout`/`shutdown_background` exist; the
  famous panic "Cannot drop a runtime in a context where blocking is not
  allowed" comes from dropping a runtime *inside* an async context.
- **CPU-bound work** — tokio's own guidance: use `spawn_blocking` for
  "short-lived blocking operations", dedicated threads for long-lived ones; "If
  your code is CPU-bound and you wish to limit the number of threads … consider
  using the rayon library … use a `oneshot` channel to send the result back";
  `spawn_blocking`'s pool limit is "very large by default", so gate parallel
  decodes with a `Semaphore` if more than one can be in flight.
  `block_in_place` panics on a `current_thread` runtime and "cannot be
  cancelled".
- **`self_update` / `reqwest::blocking`** — `self_update` 0.44 uses
  `reqwest::blocking` (its own thread + current-thread runtime internally);
  reqwest's docs: "the functionality in `reqwest::blocking` must *not* be
  executed within an async runtime, or it will panic when attempting to block …
  consider changing that caller to use `tokio::task::spawn_blocking`". So the
  update check/install must run inside `spawn_blocking` (or on a plain thread),
  never in an async task.
- **Testing** — `#[tokio::test]`: "The default test runtime is single-threaded.
  Each test gets a separate current-thread runtime"; `start_paused = true`
  (`test-util` feature) freezes/auto-advances `tokio::time`; a runtime can be
  built inline with `Builder::new_current_thread().enable_all().build()`.
  `select!` cancellation-safety: `mpsc::recv`, `watch::changed`,
  `JoinSet::join_next`, `CancellationToken::cancelled` are cancel-safe.

## (e) eframe/egui and tokio

- The frame loop is synchronous. eframe 0.34 (2026-03-26) replaced `App::update`
  with `fn logic` and `fn ui` (#7775); 0.36 (2026-08-05) never runs an egui pass
  when nothing is shown (#8387). `App::logic` is "Called once before each call
  to `Self::ui`, and additionally also called when the UI is hidden, but
  `egui::Context::request_repaint` was called … You may NOT show any ui or do
  any painting during the call to `Self::logic`… To force another call to
  `Self::logic`, call `egui::Context::request_repaint` at any time (e.g. from
  another thread)." That makes `logic()` the natural place to drain messages —
  and it keeps working while the window is minimised.
- `egui::Context` "is cheap to clone, and any clones refers to the same mutable
  data (`Context` uses refcounting internally)"; it is `Send + Sync` (asserted by
  a unit test in `context.rs`). `request_repaint`: "If called from outside the
  UI thread, the UI thread will wake up and run, provided the egui integration
  has set that up … (this will work on `eframe`)". `request_repaint_after(d)`
  exists for timers (only the smallest requested duration wins).
- `eframe::run_native` runs the winit event loop on the calling (main) thread
  and returns only on exit; the runtime therefore has to live *beside* it.
- egui's README FAQ: "If you call `.await` in your GUI code, the UI will freeze
  … keep the GUI thread non-blocking and communicate with any concurrent tasks
  (`async` tasks or other threads) with something like: Channels (e.g.
  `std::sync::mpsc::channel`). Make sure to use `try_recv` so you don't block
  the gui thread! / `Arc<Mutex<Value>>` / `poll_promise::Promise` /
  `eventuals::Eventual` / `tokio::sync::watch::channel`".
- The maintainer-recommended tokio shape (discussion #521): "Run the async
  runtime off the main thread … Build the runtime in the main thread and hold an
  `EnterGuard` on the stack so that you can call `tokio::spawn()` from your UI
  … you can use an `mpsc` channel and poll the receiver with `try_recv()` in the
  GUI render loop." Discussion #2010 suggests `poll_promise` for the simplest
  cases. `examples/external_eventloop_async` shows the *other* option — a
  current-thread runtime with `LocalSet` pumping eframe on the same thread via
  `pump_eframe_app` — explicitly Linux-only and more invasive than we need.
- egui's own demo pattern (`crates/egui_demo_app/src/apps/http_app.rs`):
  `let ctx = ui.ctx().clone(); ehttp::fetch(request, move |response| { …
  ctx.request_repaint(); // wake up UI thread … sender.send(resource); });` —
  callback sends a message, then wakes the UI. Same shape as our
  `screenshot_watcher.rs` and `updater.rs`.
- Helper crates: `egui_inbox` 0.13.0 (2026-08-06, egui ≥ 0.36) — "Channel to
  send messages to egui views from async functions, callbacks, etc. … Will
  automatically call `request_repaint()` on the `Ui` when a message is received"
  (`UiInbox::new()`, `sender()`, `read(ui)`); optional `tokio` feature. Nice,
  but it couples the channel to egui and is ~40 lines to write ourselves.
  `poll-promise` 0.3.0 — "start a background operation and then ask 'are we
  there yet?' on each subsequent frame"; last release 2023-08, last commit
  2023-09 — effectively unmaintained, and its README itself warns "decisions
  about execution environments and thread blocking should be left to the app".
  `ehttp` 0.7.1 is alive but we already have `reqwest`.

## What the app does today (for contrast)

Three independent hand-rolled effect loops, each `std::sync::mpsc` +
`std::thread::spawn` + `ctx.request_repaint()` + per-frame `try_recv`:

- Image decode: `main.rs::request_image` spawns a thread per path, hands the
  receiver to `AssetCache::request`; `poll_all_assets` calls `AssetCache::poll`
  (`try_recv`) each frame and drops results for non-active maps ("Non-active
  results are dropped without upload"). Cancellation is implicit: the decode runs
  to completion and the result is discarded.
- Screenshot watcher: `notify` callback thread parses filenames, `tx.send` +
  `request_repaint`; `poll` drains to the newest position (latest-value
  semantics, i.e. a `watch`).
- Updater: two `thread::spawn`s using `self_update`'s blocking reqwest client,
  event/command channels drained in `Updater::poll`.
- Demo walker: pure sync `poll(now)`; the reference fake position source.
- `fetch_maps` already uses `tokio` (`JoinSet`, `Semaphore`, `tokio::fs`) with
  a multi-thread runtime.

The proposal below is these three loops unified into one, with the *decision*
part moved into the model and the *thread* part moved into one executor.

## Recommendation

"Sane, not over-engineered": one sync model, one async executor, one channel,
one runtime. No `dyn` async, no proc-macro crates, no helper crates.

### 1. Port trait shape

```rust
// core::ports — sync driven ports: injected, dyn-compatible, called by the model
pub trait Clock { fn now(&self) -> Instant; }
pub trait MapCatalog { fn maps(&self) -> &[Map]; fn by_name(&self, n: &str) -> Option<&Map>; }

// core::ports — async driven ports: called only by the executor, never by the model.
// Desugared form so `Send` is pinned (the lint's advice); impls may still write `async fn`.
pub trait ImageDecoder: Send + Sync + 'static {
    fn decode(&self, path: &ImagePath) -> impl Future<Output = Result<Bc7Image, DecodeError>> + Send;
}
pub trait ReleaseFeed: Send + Sync + 'static {
    fn latest(&self, current: &Version) -> impl Future<Output = Result<Option<Release>, FeedError>> + Send;
    fn install(&self, release: &Release) -> impl Future<Output = Result<(), FeedError>> + Send;
}
/// A stream of Player Positions; adapters: notify-backed watcher, demo walker.
pub trait PositionSource: Send + 'static {
    fn next(&mut self) -> impl Future<Output = Option<PlayerPosition>> + Send;
}
```

Adapters implement these with `async fn` (the `reqwest`/`self_update` ones wrap
the blocking call in `spawn_blocking`; the decoder does `spawn_blocking(move ||
load_and_decode_image(&path))`). Fakes are `struct FakeDecoder(HashMap<..>)`
with `async fn decode` returning canned results — no mocking crate.

### 2. Model: sync, returns effects

```rust
pub enum Msg {
    SelectMap(MapName), Tick(Instant), PositionFix(PlayerPosition),
    ImageDecoded { map: MapName, result: Result<Bc7Image, DecodeError> },
    ReleaseChecked(Result<Option<Release>, FeedError>), UpdateInstalled(Result<(), FeedError>),
    UpdateNow, /* … */
}
pub enum Effect {
    DecodeImage { map: MapName },       // executor cancels any other in-flight decode
    CancelDecode { map: MapName },
    CheckForUpdate, InstallUpdate(Release),
    WatchPositions,                     // start (once) the position stream
    Notify(Notification),               // toast; still an effect, executed by the app
}
pub struct Model { /* selection, viewport, overlays, image state per map, … */ }
impl Model {
    pub fn handle(&mut self, msg: Msg, ports: &dyn SyncPorts) -> Vec<Effect> { /* pure */ }
}
```

The model keeps `AssetCache`'s state machine (Loading/Decoded/Uploaded/Error)
but without the `Receiver` inside it: `Loading` becomes a plain marker, and
`ImageDecoded { map, .. }` for a map that is no longer selected is dropped (the
current "non-active results are dropped" rule, now a unit-testable branch).

### 3. Executor: the only async code (in core, tokio allowed)

```rust
pub struct Executor<P: Ports> {
    ports: Arc<P>,
    handle: tokio::runtime::Handle,
    tasks: tokio::task::JoinSet<()>,
    decode: Option<(MapName, tokio_util::sync::CancellationToken)>,
    tx: tokio::sync::mpsc::UnboundedSender<Msg>,
    wake: Arc<dyn Fn() + Send + Sync>,          // app passes `move || ctx.request_repaint()`
}
impl<P: Ports> Executor<P> {
    pub fn run(&mut self, effect: Effect) {
        match effect {
            Effect::DecodeImage { map } => {
                if let Some((_, tok)) = self.decode.take() { tok.cancel(); }
                let tok = CancellationToken::new();
                self.decode = Some((map.clone(), tok.clone()));
                let (ports, tx, wake) = (self.ports.clone(), self.tx.clone(), self.wake.clone());
                self.tasks.spawn_on(async move {
                    if let Some(result) = tok.run_until_cancelled(ports.decoder().decode(&map.image_path())).await {
                        let _ = tx.send(Msg::ImageDecoded { map, result }); wake();
                    }
                }, &self.handle);
            }
            Effect::CheckForUpdate => { /* spawn ports.releases().latest(..) -> Msg::ReleaseChecked */ }
            Effect::WatchPositions => { /* spawn loop: while let Some(p) = src.next().await { tx.send(PositionFix(p)); wake() } */ }
            /* … */
        }
    }
}
```

`JoinSet` gives free shutdown ("dropped … all tasks … immediately aborted");
`run_until_cancelled` drops the awaiting future on map switch. Because the
decode itself is `spawn_blocking` (uncancellable once started), the stale result
is *also* filtered by the model — belt and braces, both cheap.

### 4. App crate: runtime ownership and the frame loop

```rust
fn main() -> eframe::Result {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    let _enter = rt.enter();                          // tokio::spawn / Handle::current() work on this thread
    eframe::run_native("tarkov-map", opts, Box::new(move |cc| {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let ctx = cc.egui_ctx.clone();
        let executor = Executor::new(Arc::new(RealPorts::new()), rt.handle().clone(), tx,
                                     Arc::new(move || ctx.request_repaint()));
        Ok(Box::new(App { model: Model::new(..), executor, rx, .. }))
    }))?;
    rt.shutdown_timeout(Duration::from_secs(2));      // don't hang on a stuck blocking task at exit
    Ok(())
}
impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() { self.dispatch(msg); }
        // GPU upload of Decoded images stays here (needs `frame.wgpu_render_state()`), as does toasts.
    }
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        for msg in view::draw(ui, &self.model) { self.dispatch(msg); }   // UI intents are Msgs too
    }
}
impl App {
    fn dispatch(&mut self, msg: Msg) {
        for effect in self.model.handle(msg, &self.sync_ports) { self.executor.run(effect); }
    }
}
```

`Runtime` is dropped after `run_native` returns (main thread, outside any async
context — no "Cannot drop a runtime" panic); `shutdown_timeout` bounds the wait
on `spawn_blocking` work. `Handle` is `Clone + Send`, so the executor may live
in core without owning the runtime. If a `Msg` needs a timer (demo walker step,
toast expiry), use `ctx.request_repaint_after` from the app or `tokio::time`
inside the executor — not both.

### 5. Testing

- **Model:** `#[test]`, no runtime. `let fx = model.handle(Msg::SelectMap(woods),
  &fake_sync_ports); assert!(matches!(fx[..], [Effect::DecodeImage{..}]))`; then
  `model.handle(Msg::ImageDecoded{ map: customs, .. })` and assert it is ignored.
  This is where `AssetCache`'s existing tests move (they already inject a
  ready channel — that becomes injecting a `Msg`).
- **Executor:** `#[tokio::test]` (current-thread by default; `start_paused =
  true` if timers are involved) with `Executor::new(Arc::new(FakePorts), Handle::current(), tx, Arc::new(|| {}))`;
  run an effect, `rx.recv().await`, assert the `Msg`. Cancellation test: run
  `DecodeImage(a)` with a fake decoder that awaits a `Notify`, run
  `DecodeImage(b)`, release, assert only `b`'s message arrives.
- **Adapters:** integration tests where worthwhile (filename parsing is pure and
  already tested; the notify watcher and reqwest paths are thin).

### 6. What we deliberately do not do

- No `Box<dyn AsyncPort>` (`async-trait`/`dynosaur`) — the executor is generic
  and there is one adapter set. Revisit only if a `dyn` port becomes necessary;
  `dynosaur` is the endorsed interim, `async-trait` the boring one.
- No async in the model, no `block_on` in the UI thread, no `blocking_recv`.
- No `poll-promise` (unmaintained) or `egui_inbox` (couples the channel to egui;
  our `wake` closure does the same in one line and keeps core egui-free).
- No per-effect thread; `spawn_blocking` is bounded to the handful of decodes and
  updater calls we have.
- No second runtime: `self_update`/`reqwest::blocking` spin their own internal
  thread; that is fine inside `spawn_blocking`.

## Notes for the ADR (non-normative)

- Candidate glossary terms surfacing here: **Msg** (anything that changes the
  model: UI intent or effect result), **Effect** (a description of side-effecting
  or slow work the model asks for), **Executor** (the single async component
  that runs Effects against async ports and returns Msgs), **wake** (the
  executor's callback that the app binds to `request_repaint`).
- If the workspace ever needs a headless CLI over core, the same
  `Model` + `Executor` pair runs under `Runtime::block_on` with a `wake` that
  does nothing — the design already supports it.
- Watch RTN and the 2026 "afidt-box" goal; if native `dyn` async lands the
  generic executor could become `dyn`-based with no other change.
