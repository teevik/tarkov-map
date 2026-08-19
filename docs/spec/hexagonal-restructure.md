# Architecture spec: hexagonal core + model-driven restructure

This is the destination artifact of the wayfinder map
[Wayfinder: hexagonal core + model-driven restructure](https://github.com/teevik/tarkov-map/issues/35).
It indexes every decision the map made, in the order an implementer needs them,
and is meant to be handed straight to `/to-tickets` / `/implement`.

**How to read it.** Each section states the decision and points at where the
detail lives — an ADR under `docs/adr/`, a research note under `docs/research/`,
or the resolving ticket's resolution comment. The spec is the authority where it
is explicit; the ADRs are authoritative for the *why*; ticket comments hold the
full grilling record; the prototype branch
[`prototype/core-skeleton`](https://github.com/teevik/tarkov-map/tree/prototype/core-skeleton)
is a reference implementation, **not** an authority — where it and this spec
differ, the spec wins. Domain vocabulary (Map, Projection, Bounds, Player
Position, Position Source, Freshness, Overlay, Trail, Map Suggestion, Viewport,
Map Transition, Notification, Bundle, Refresh, …) is defined once in
[`CONTEXT.md`](../../CONTEXT.md) and used here without redefinition.
Architecture vocabulary — **Message**, **Effect**, **Effect Runner** — is
defined in [ADR-0002](../adr/0002-sync-model-effects-out-async-in-one-runner.md).

**Baseline.** The migration starts from `main` after the richer-overlays
implementation ([#70](https://github.com/teevik/tarkov-map/issues/70)) lands.
The overlay types that effort adds to today's `Map` (Sniper Zone, Minefield,
Boss Spawn, Transit, Switch, BTR Stop) are carried into the core `Map`; the
field lists in this spec and in ADR-0003 are a **floor, not a ceiling**.

---

## 1. Goals and shape

Restructure `tarkov-map` into a Cargo workspace with

- a **testable, model-driven core** — domain types, a synchronous application
  model, driven ports, and one Effect Runner;
- an **adapters** crate implementing the ports against the real world
  (filesystem, embedded assets, GitHub releases, `notify`);
- a **thin egui app** crate that owns pixels, input, the GPU and the tokio
  runtime, and nothing else;
- plus two supporting members: the `bc7z` image-container format and
  `fetch` (the `fetch_maps` anti-corruption layer).

Guiding constraints fixed during charting (see the map's Notes): ports only
where a second implementation or a test needs one; hand-rolled fakes, no
mockall; plain `#[test]`; tokio allowed in core but confined to the runner;
Alice Ryhl-style ownership (sync `handle`, async only at the edges, channels
over locks); `eros` for error composition outside core; blessed.rs crates;
behaviour improvements welcome (§13).

---

## 2. Workspace layout — [ADR-0004](../adr/0004-cargo-workspace-with-the-app-as-root-package.md)

Resolved by [Decide: workspace layout, crate names, module structure, fetch_maps placement](https://github.com/teevik/tarkov-map/issues/49),
amended by [Decide: fetch_maps as anti-corruption layer](https://github.com/teevik/tarkov-map/issues/53).
Mechanics: [`docs/research/workspace-mechanics.md`](../research/workspace-mechanics.md).

| Path | Package | Role | Depends on |
|---|---|---|---|
| `/` (root) | `tarkov-map` | egui app — **stays the workspace root package** so release-please, `v*` tags, `CHANGELOG.md`, `release.yml`, `build.rs`/winres and the artefact path are untouched | adapters, core, eframe/egui, tokio, `dirs`, `log`/`env_logger` |
| `crates/core` | `tarkov-map-core` | domain + application model + ports + Effect Runner | euclid, tokio (runner only), serde, futures — **nothing** that knows egui, ron, rust-embed, the filesystem, or eros |
| `crates/adapters` | `tarkov-map-adapters` | driven-port implementations, bundled-catalog loader | core, bc7z, rust-embed, ron, notify, self_update, eros (`default-features = false`) |
| `crates/bc7z` | `tarkov-map-bc7z` | the `.bc7z` container format; `encode` feature gates `intel_tex_2` | zstd |
| `crates/fetch-maps` | `tarkov-map-fetch` | `fetch_maps`, **lib + bin**; `[[bin]] required-features = ["encode"]`, `encode = ["tarkov-map-bc7z/encode"]` | core, bc7z, reqwest, resvg, clap, eros (`context`+`backtrace`) |

- Dependency direction is one-way: app → adapters → core; adapters → bc7z;
  fetch → core + bc7z. Core depends on no member.
- Members are `publish = false`, version frozen at `0.1.0`; only the root app
  is versioned/released. `[workspace.package]` shares `edition`/`license`
  (never `version`); `[workspace.dependencies]` and `[workspace.lints]` shared.
- **Assets stay at the repo root** (`assets/maps.ron`, `assets/maps/*.bc7z`,
  icon). The rust-embed derive and the `maps.ron` parse live in adapters with
  `../../assets` paths; fetch's `repo_path` default is `../../assets`.
- **Core modules**: `domain/` (`map`, `position`, `geometry`/spaces, `overlay`,
  `notification`, `release`, `viewport`), `model/` (`Model`, `Msg`, `Effect`,
  `viewport`, `settings`, `suggestion`, `view` queries, `tests/`), `ports/`
  (one file per port, `fakes/` behind the `fakes` feature), `runner.rs` (the
  only tokio user), `testing.rs` (synthetic catalog + harness, `cfg(test)`/`fakes`).
  Only `Model`, `Msg`, `Effect` are re-exported at the crate root.
- **App modules** (ADR-0005): `main.rs`, `app.rs`, `input.rs`, `map_view.rs`,
  `sidebar.rs`, `chrome.rs`, `textures.rs`, `toasts.rs`, `theme.rs`;
  `src/bin/tarkov-map/` moves to `src/`.
- **CI/release**: `ci.yml` → `cargo test --workspace` + `cargo clippy --workspace`
  (Linux); `release.yml` → `cargo build --release --locked -p tarkov-map`
  (Windows keeps the release build only). release-please config and nix untouched.
  The quality gate ([#16](https://github.com/teevik/tarkov-map/issues/16)) is already in place.

---

## 3. Domain (`core::domain`)

### 3.1 Geometry — typed coordinate spaces

Research: [`docs/research/geometry-crate-for-core.md`](../research/geometry-crate-for-core.md)
([ticket](https://github.com/teevik/tarkov-map/issues/37)).

- `euclid` 0.22 with the `serde` feature is core's only geometry dependency.
  Three unit types: `Game` (tarkov x/z metres), `Image` (pixels of the bundled
  Main Floor image), `Screen` (egui points). `Point2D<f64, _>`,
  `Vector2D<f64, _>`, `Size2D<f64, _>`, `Box2D<f64, _>`, `Angle<f64>`,
  `Transform2D<f64, From, To>` composing via `.then()`. f32 conversion happens
  at the egui edge only.
- Typed aliases: `GamePos = Point2D<f64, Game>`, `ImagePos = Point2D<f64, Image>`.

### 3.2 `Map`, `MapCatalog`, Projection — [ADR-0003](../adr/0003-projection-is-a-baked-affine-per-map.md)

Resolved by [Decide: domain Map type and maps.ron schema](https://github.com/teevik/tarkov-map/issues/46);
oracle maths in [`docs/research/tarkov-dev-coordinate-oracle.md`](../research/tarkov-dev-coordinate-oracle.md)
([ticket](https://github.com/teevik/tarkov-map/issues/39)).

```rust
pub struct MapId(String);            // today's normalized_name; the key everywhere
pub struct MapImageKey(String);      // verbatim asset-relative path, e.g. "maps/customs.bc7z" (ADR-0006)

pub struct Map {
    pub id: MapId,
    pub name: String,                                   // display name
    pub image: MapImageKey,
    pub image_size: Size2D<f64, Image>,                 // real rendered/stitched pixel size
    pub game_to_image: Transform2D<f64, Game, Image>,   // the Projection — one baked affine, tiles *and* SVG
    pub bounds: Box2D<f64, Game>,                       // playable extent (Map Suggestion's inside-test)
    pub attribution: Option<Attribution>,               // { name, link: Option<String> }
    pub labels: Vec<Label>,                             // { position, text, rotation: Angle (default 0), size: f64 (default 40) }
    pub spawns: Vec<Spawn>,                             // { position }  — PMC player spawns by construction
    pub extracts: Vec<Extract>,                         // { name, faction: Faction { Pmc, Scav, Shared }, position }
    // carried over from the richer-overlays baseline (#70), one Vec<T> per Overlay kind, same shape rule:
    pub sniper_zones: Vec<SniperZone>, pub minefields: Vec<Minefield>, pub boss_spawns: Vec<BossSpawn>,
    pub transits: Vec<Transit>, pub switches: Vec<Switch>, pub btr_stops: Vec<BtrStop>,
}
pub struct MapCatalog { maps: Vec<Map> }   // ordered: list order = sidebar order = Ctrl+1..9 order
```

- All fields pub (ron deserialisation bypasses constructors; fetch builds
  literals). **No `Option`** on any collection or geometry field. Dropped from
  today's type: `coordinate_rotation`, `transform`, the `rotation == 270`
  branch, `logical_size`, `alt_maps`, `Spawn.sides/categories`,
  `Label.top/bottom`, height on spawns/extracts.
- **Projection API** on `Map`: `project(GamePos) -> ImagePos`,
  `contains(GamePos) -> bool` (bounds), `metres_per_pixel()`, and the on-image
  heading derived by pushing `(sin yaw, cos yaw)` through the affine's linear
  part and normalising — no rotation field.
- **Bounds**: SVG maps keep upstream `bounds`; tile maps get bounds derived by
  unprojecting the tile rectangle (fixes Icebreaker's copied-from-Factory bounds).
- **`MapCatalog::try_new(Vec<Map>) -> Result<MapCatalog, CatalogError>`** is
  the **one validator** (ADR-0006): non-empty, unique ids, positive image
  sizes, invertible affines, no dangling Transit targets; `CatalogError` names
  Map, collection, entry and the failed invariant. Run by fetch before writing
  and by adapters after loading; failure on load is fatal (ADR-0001).
  Lookup by `MapId`, index by position.
- **Schema**: `maps.ron` is domain-shaped — `Serialize`/`Deserialize` derived
  on the domain types, positions serialise as `(x, y)`, **no version field**
  (regenerated every release), no DTO layer. `ron` parsing lives in adapters.
- Display order is **data in `maps.ron`** written by fetch from its fixed table:
  Customs, Factory, Woods, Shoreline, Interchange, Reserve, Lighthouse, Streets
  of Tarkov, Ground Zero, then The Lab, The Labyrinth, Icebreaker, Terminal;
  unknown maps appended alphabetically. No `order` field.
- **Known visible change**: Reserve and Icebreaker markers move to the correct
  place (today's port is wrong for both).

### 3.3 `PlayerPosition` and the screenshot parser

Resolved by [Decide: Player Position domain — parser, sources, staleness](https://github.com/teevik/tarkov-map/issues/44).

```rust
pub struct PlayerPosition {
    pub ground:   Point2D<f64, Game>, // game x, z
    pub height:   f64,                // game y — displayed only
    pub heading:  Angle<f64>,         // radians, yaw; convention documented once
    pub taken_at: SystemTime,         // screenshot mtime; adapter falls back to SystemTime::now()
}
pub struct ScreenshotStamp { ground, height, heading }
pub fn parse_screenshot_name(&str) -> Result<ScreenshotStamp, ScreenshotNameError>; // pure, no I/O, no regex
pub enum ScreenshotNameError { NotAScreenshot, BadNumber { field: &'static str } }
```

- Hand-parsed (split on `_` then `, `). Only `.png` (case-insensitive) files
  are considered; parse failures are logged at debug by the source and ignored
  — never a Notification. The raw quaternion is discarded after yaw extraction;
  TarkovMonitor's yaw formula preserved verbatim
  (`siny_cosp = 2(w·y + x·z)`, `cosy_cosp = 1 − 2(z² + y²)`, `atan2`) with
  golden tests (identity → 0, ±90° about Y → ±π/2, the doc-comment example filename).
- `Freshness { Live, Stale }` is a **derived query** with a 120 s core constant;
  nothing stored, no tick.

### 3.4 Other domain types

- `Notification { id: NotificationId, severity: Info|Warning|Error, text: String, raised_at: SystemTime, action: Option<NotificationAction> }`,
  `enum NotificationAction { InstallUpdate, Restart }` ([updater ticket](https://github.com/teevik/tarkov-map/issues/51)).
- `Release { version: String, tag: String }`; `UpdateState { Idle, Checking, Available(Release), Installing(Release), Installed(Release) }`.
- `Settings { selected_map: Option<MapId>, hidden_overlays: BTreeSet<OverlayKind>, screenshots_dir: Option<PathBuf> }`
  — serde derives, `#[serde(default)]`, unversioned; `hidden_overlays` (not
  the visible set) is persisted so new kinds default to on.
- `OverlayKind { Labels, Spawns, PmcExtracts, ScavExtracts, SharedExtracts, PlayerMarker, Trail, … }`
  — the overlays effort's kinds (Sniper zones, Minefields, per-Mob, Transits,
  Switches, BTR stops) append. Only `Labels` hidden by default.
- `MapImageData { pixel_size: Size2D<u32, Image>, padded_size: Size2D<u32, Image>, bc7_blocks: Vec<u8> }`
  — plain data (today's `Bc7Image` without parsing); rides opaquely from the
  decoder through `Msg::ImageDecoded` to `Effect::PresentImage`.

---

## 4. Application model (`core::model`) — [ADR-0002](../adr/0002-sync-model-effects-out-async-in-one-runner.md)

Resolved by [Decide: application model — state, messages, effects, tick](https://github.com/teevik/tarkov-map/issues/47),
extended by [Decide: updater, settings persistence, and notifications wiring](https://github.com/teevik/tarkov-map/issues/51)
and amended by [Prototype: core crate skeleton](https://github.com/teevik/tarkov-map/issues/54).

### 4.1 Shape

```rust
impl Model {
    pub fn new(catalog: MapCatalog, settings: Settings, default_screenshots_dir: Option<PathBuf>,
               options: Options { check_for_updates: bool }, now: SystemTime) -> (Model, Vec<Effect>);
    pub fn handle(&mut self, msg: Msg, now: SystemTime) -> Vec<Effect>;   // the ONLY mutation path; sync, never awaits
}
```

`new` emits the initial `DecodeImage(selected)`, `WatchScreenshots(effective_dir)`
(or sets `Tracking::Off(NoDocumentsFolder)`), and `CheckForUpdate` iff enabled.
The model reads no ports: the catalog is plain data, time is a `now` argument on
`handle` and on every time-dependent query. **No tick, no runner timers.**

### 4.2 State

```rust
pub struct Model {
    catalog: MapCatalog,
    selected: MapId,                          // always valid; unknown/None in settings → first in catalog
    viewport: Viewport,                       // { zoom: f64 in [1, 10] (1 = fit), center: Point2D<f64, Image> }
    image: MapImage,                          // selected map: Loading { since } | Ready { since } | Failed
    outgoing: Option<MapId>,                  // previous image kept until the fade completes
    positions: VecDeque<PlayerPosition>,      // cap 20; back() = current, the rest = Trail
    tracking: Tracking,                       // Off(WatchError) | Starting | Waiting(SourceKind) | Positioned(SourceKind)
    suggestion: Option<MapId>, suggestion_muted: bool,
    settings: Settings, default_screenshots_dir: Option<PathBuf>,
    notifications: Vec<Notification>,         // newest last, no cap
    presentation_unsupported_reported: bool,  // BC-unsupported raised once per session
    update: UpdateState,
}
```

### 4.3 Messages (`Msg`)

UI intents: `SelectMap(MapId)`, `SelectMapByIndex(usize)`,
`ZoomBy { factor, anchor: Option<Point2D<Image>> }`, `PanBy(Vector2D<Image>)`,
`ResetView`, `CenterOnPlayer`, `ToggleOverlay(OverlayKind)`,
`AcceptSuggestion`, `DismissSuggestion`, `SetScreenshotsDir(Option<PathBuf>)`
(None = default), `ResetSettings`, `DismissNotification(NotificationId)`,
`InstallUpdate`, `Restart`.
Runner results: `PositionEvent(PositionEvent)`, `ImageDecoded { map, image: MapImageData }`,
`ImageDecodeFailed { map, error }`, `UpdateAvailable(Release)`, `UpToDate`,
`UpdateInstalled(Release)`, `UpdateFailed(ReleaseFeedError)`.
App-originated: `ImagePresentFailed { reason }` (GPU upload), `RestartFailed(String)`
(the intercepted `Restart` could not spawn) — the only two.

Keyboard bindings (`Ctrl+1..9`, `+`/`-`, `R`, `C`), Ctrl-held badges, the
folder picker and window chrome are app detail that *produce* Messages.

### 4.4 Effects

`DecodeImage { map: MapId, image: MapImageKey }` (carries what the port needs;
the runner holds no catalog) · `PresentImage { map, image: MapImageData }`
(**app-intercepted**) · `WatchScreenshots(PathBuf)` · `StopWatching` ·
`PersistSettings(Settings)` · `CheckForUpdate` · `InstallUpdate(Release)` ·
`Restart` (**app-intercepted**). `wake` is runner plumbing, not an Effect.
There is no `Effect::Log`, no `Effect::After`, no `ReleaseImage`.

### 4.5 Rules

- **Selection**: `SelectMap` of the selected map is a no-op. A real switch:
  viewport reset, `image = Loading { since: now }`, `outgoing = previous`,
  Trail reduced to the newest position (kept), suggestion cleared + unmuted
  then re-evaluated, settings updated → `[DecodeImage, PersistSettings]`.
  `SelectMapByIndex(n)` beyond the catalog is a no-op; `AcceptSuggestion` =
  `SelectMap(candidate)`.
- **Viewport**: `ZoomBy` clamps to `[1, 10]` and keeps `anchor` fixed under the
  cursor (`center' = anchor − (anchor − center)/factor`; `None` = about centre);
  `CenterOnPlayer` sets `center = project(current)`, raising zoom to 2.5 only
  from 1, no-op without a position. The app never moves the Viewport itself
  (**no follow mode**).
- **Image lifecycle**: `ImageDecoded` for the selected map → `Ready { since: now }`
  + `PresentImage`; for any other map dropped with no effects (stale-decode
  drop). `ImageDecodeFailed`/`ImagePresentFailed` → `Failed` + Notification.
  Placeholder delay (300 ms) and fade-in (220 ms) are queries off `since`;
  `outgoing` clears when the fade completes.
- **Positions**: identical or older-than-current ignored; `Started(kind)` clears
  positions and Trail, sets `Waiting(kind)`; `Failed(err)` → `Tracking::Off(err)`
  + Notification. On each accepted position: Trail capped at 20; suggestion
  recomputed — outside selected `bounds` and inside exactly one other Map →
  set unless muted; inside selected → clear + unmute; zero/several → none.
  `DismissSuggestion` → clear + mute.
- **Settings**: every settings mutation yields exactly one `PersistSettings`;
  `SetScreenshotsDir` → `[StopWatching, WatchScreenshots(effective), PersistSettings]`;
  `ResetSettings` → defaults, first map selected, rewatch, persist — in place,
  no restart.
- **Notifications**: the model raises each user-relevant error (decode,
  presentation, watcher, update install, restart); identical text not
  re-raised while queued; BC-unsupported once per session; the app's toast
  layer sends `DismissNotification` on timeout/close (model has no durations).
- **Updater** (model-owned): `UpdateAvailable` → `Available` + sticky Info
  "Update available: vX" with action `InstallUpdate`; `InstallUpdate` (only
  from `Available`) → `Installing`, dismisses that Notification, raises Info
  "Downloading update…", emits `Effect::InstallUpdate`; `UpdateInstalled` →
  `Installed` + sticky "Updated to vX. Restart to apply." with action
  `Restart`; `UpdateFailed` → back to `Available` + Error; `UpToDate` → `Idle`
  + Info "Already up to date (vX)"; `Restart` → `Effect::Restart`;
  `RestartFailed` → Error, `Installed` retained. `CheckForUpdate` once at
  startup iff `Options.check_for_updates` (app passes `false` under
  `cfg!(debug_assertions)` / `--no-update-check`); no periodic recheck, no
  manual button; a startup check that finds nothing is silent.

### 4.6 Queries and the rendering seam — [ADR-0005](../adr/0005-core-yields-image-space-items-app-paints.md)

Resolved by [Decide: rendering boundary — what the egui crate keeps vs what core owns](https://github.com/teevik/tarkov-map/issues/50).

- Borrowed `&Model` queries (`core::model::view`): `overlay_items()`,
  `transition_opacity(now)`, `show_placeholder(now)`, `in_transition(now)`,
  `freshness(now)`, `offered_overlays()` (kinds with data on the selected Map,
  plus `PlayerMarker` and `Trail` always), `notifications()`, `selected_map()`,
  `viewport()`, `tracking()`, `suggestion()`, `update()`. Everything in
  **Image space**; no snapshot type.
- **`ViewFrame`** (`core::domain::viewport`): `Viewport::frame(image_size, screen_size: Size2D<Screen>) -> ViewFrame { image_to_screen, screen_to_image }`
  — the only bridge to Screen space; pure, golden-tested.
- **`OverlayItem`** yielded in draw order (areas → markers → Trail → Player
  marker), each with its `OverlayKind` and Image-space geometry:
  `Label { pos, text, rotation, size }`, `Spawn { pos }`, `Extract { pos, name, faction }`,
  `Trail { pos, index }` (0 = oldest), `PlayerMarker { pos, heading, freshness }`
  (heading already through the Projection); the overlays effort's kinds
  (outlines, Boss Spawn, Transit, Switch, BTR Stop) are further variants. The
  app's painter is one `match`.
- **Constants**: core owns zoom `[1, 10]`, centre zoom 2.5, placeholder 300 ms,
  fade 220 ms, freshness 120 s, Trail cap 20. The app owns scroll zoom speed,
  points per notch, key step, marker/label px sizes, colours, sidebar width,
  toast timeouts.

---

## 5. Ports (`core::ports`)

Resolved by [Decide: ports catalogue — final list, signatures, error unions, fakes](https://github.com/teevik/tarkov-map/issues/48).
Exactly four driven ports; every port fn takes `&self`; traits are `Send + Sync + 'static`.

| Port | Sync/async | Signature | Error | Real adapter | Fake |
|---|---|---|---|---|---|
| `PositionSource` | async **stream** | `fn observe(&self, dir: PathBuf) -> impl Stream<Item = PositionEvent> + Send + 'static` | none on the fn; `PositionEvent::Failed(WatchError)` is an item. `WatchError { NoDocumentsFolder, FolderMissing(PathBuf), WatchFailed(Box<dyn Error + Send + Sync>) }` | `ScreenshotWatcher` (`notify`; newest existing PNG first, then Create events; dedupes same path twice), `DemoPositionSource` (fixed map — Customs — ignores `dir`); `enum AnyPositionSource { Screenshots, Demo }` chosen in `main` | `FakePositionSource` = scripted `stream::iter` |
| `ImageDecoder` | sync (`spawn_blocking`) | `fn decode(&self, key: &MapImageKey) -> Result<MapImageData, ImageDecodeError>` | `ImageDecodeError { key, source }` — one struct | `EmbeddedImageDecoder` — rust-embed (fs in debug, embedded in release) + bc7z unpack | `FakeDecoder` — scripted per key, `Semaphore`-gated for cancellation tests |
| `ReleaseFeed` | sync (`self_update` is `reqwest::blocking`; `spawn_blocking`) | `fn latest(&self) -> Result<Option<Release>, ReleaseFeedError>` (None = up to date) · `fn install(&self, &Release) -> Result<(), ReleaseFeedError>` | `ReleaseFeedError { source }` | `GithubReleaseFeed` (self_update; compatible-bump-first preference lives here) | `FakeReleaseFeed` |
| `SettingsStore` | sync (`spawn_blocking`) | `fn load(&self) -> Result<Option<Settings>, SettingsStoreError>` · `fn save(&self, &Settings) -> Result<(), SettingsStoreError>` | `SettingsStoreError { source }` | `FileSettingsStore` — `settings.ron` under `dirs::config_dir()/tarkov-map/` | `InMemorySettingsStore` |

- `PositionEvent { Started(SourceKind), Position(PlayerPosition), Failed(WatchError) }`,
  `SourceKind { Screenshots, Demo }`. The initial newest screenshot is just the
  stream's first `Position` item.
- **`Ports` bundle**: `trait Ports: Send + Sync + 'static { type Decoder: ImageDecoder; type Positions: PositionSource; type Releases: ReleaseFeed; type Settings: SettingsStore; fn decoder(&self) -> &…; fn positions(&self) -> …; fn releases(&self) -> …; fn settings(&self) -> …; }`.
  Runner holds `Arc<P>`. `RealPorts` in the app crate, `FakePorts` in
  `core::ports::fakes` behind `#[cfg(any(test, feature = "fakes"))]`.
- **Not ports**: `Clock` (dropped — `now` by value, adapters call
  `SystemTime::now()`), `MapCatalog` (plain data; the bundled loader is a plain
  fn in adapters), `Notifier`, `Wake`, a folder picker (app concern, e.g. `rfd`).
- **Failure handling**: `SettingsStore::save` failure → log-only;
  `load` failure at startup → log + `Settings::default()` (missing file is
  `Ok(None)`); `ImageDecodeError` / install failure → Notification via the
  model; `ReleaseFeed::latest` failure → log-only in the runner (no Message).

---

## 6. Effect Runner (`core::runner`) — [ADR-0002](../adr/0002-sync-model-effects-out-async-in-one-runner.md)

Resolved by [Decide: how async/tokio shapes the core](https://github.com/teevik/tarkov-map/issues/45);
research: [`docs/research/async-ports-and-effects.md`](../research/async-ports-and-effects.md).

- `Runner<P: Ports>` — generic, the only tokio user in core. Takes a tokio
  `Handle`, an `Arc<P>`, the `mpsc::UnboundedSender<Msg>` and a
  `wake: Arc<dyn Fn() + Send + Sync>`; calls `wake` after every send.
- `run(effect)` matches the enum: `DecodeImage` → `spawn_blocking(decode)` →
  `ImageDecoded`/`ImageDecodeFailed`; `WatchScreenshots(dir)` → spawn
  `observe(dir)` under a `CancellationToken`, forward items as
  `PositionEvent`; `StopWatching` → cancel; `PersistSettings` → coalesced save;
  `CheckForUpdate` → `latest()` → `UpdateAvailable`/(silent)/log;
  `InstallUpdate(r)` → `install` → `UpdateInstalled`/`UpdateFailed`.
- **Cancellation by named slot**: `decode` (one in flight; new request
  implicitly cancels the previous — and because `spawn_blocking` cannot be
  aborted, the model also drops stale `ImageDecoded`), `watch` (explicit),
  `settings` (latest-wins snapshot, at most one save in flight, re-save on
  completion if a newer snapshot arrived). All tasks in a `JoinSet`, aborted on drop.
- **Runtime ownership**: app `main` builds a 2-worker multi-thread runtime,
  holds `rt.enter()`, gives the runner a `Handle`, runs `eframe::run_native`,
  then `rt.shutdown_timeout(2 s)` (load-bearing: blocking decodes finish
  there). A headless binary would be the same pair under `block_on` with
  `wake = || {}` (not built on this route).

---

## 7. Adapters crate (`tarkov-map-adapters`)

- Port impls from §5: `ScreenshotWatcher`, `DemoPositionSource`,
  `AnyPositionSource`, `EmbeddedImageDecoder`, `GithubReleaseFeed`,
  `FileSettingsStore`.
- `load_bundled_catalog() -> eros::Result<MapCatalog>`: rust-embed
  `assets/maps.ron` → `ron` → `MapCatalog::try_new`. The `.bc7z` container
  parsing moves out of the app into `bc7z`/adapters.
- Error handling per [ADR-0001 as amended](../adr/0001-error-handling-with-eros.md):
  eros freely inside (`default-features = false`); the `impl Port` maps to the
  declared core error with `.into_inner()`, **adds no `.context`** (it would
  not survive the boundary) — port errors carry their facts as fields; adapters
  do not log.
- Default screenshots folder (`Documents/Escape from Tarkov/Screenshots`) is
  resolved in the **app** crate; adapters and core only see a concrete
  `PathBuf`. No retry / parent-dir watching when the folder is missing.

---

## 8. App crate (`tarkov-map`, root)

- `main.rs`: `env_logger`; build tokio runtime; `load_bundled_catalog()` (fatal
  on error, `main() -> eros::Result<()>`); `FileSettingsStore::load()`
  **synchronously** (error → log + defaults); resolve default screenshots dir;
  pick `AnyPositionSource` (`TARKOV_MAP_DEMO`); `Model::new(...)`; build
  `RealPorts` + `Runner`; `eframe::run_native`; `shutdown_timeout`.
  `Options.check_for_updates = !cfg!(debug_assertions) && !--no-update-check`.
- `app.rs`: `App { model, runner, rx, pending: Vec<Msg>, textures }`;
  `logic()` drains `rx.try_recv()`; `ui()` borrows `&Model`, widgets push into
  `pending`, drained at the end through one **`dispatch(msg)`** =
  `model.handle(msg, now)` → `match effect { PresentImage | Restart => app, other => runner.run(other) }`.
  Those two are the **only** app-intercepted effects.
- `textures.rs`: `PresentImage` → wgpu BC7 upload into
  `HashMap<MapId, MapTexture>`; retention derived each frame (drop any
  `MapId ∉ {selected, outgoing}`); BC-unsupported → `Msg::ImagePresentFailed`.
- `map_view.rs`: image + crossfade (`transition_opacity`, placeholder after
  300 ms) + the `OverlayItem` painter (absorbs today's `overlays.rs`); drawn at
  the image's own pixel aspect; all screen↔image conversion via `ViewFrame`.
  Label crowding/culling, marker sizes, colours are here.
- `input.rs`: drag → `PanBy`, scroll → `ZoomBy { anchor }`, `+`/`-`, `R`
  (`ResetView`), `C` (`CenterOnPlayer`), `Ctrl+1..9` → `SelectMapByIndex`.
- `sidebar.rs`: map list (Ctrl-held number badges for the first nine), Overlay
  Categories with toggles (presentation state — open headings — app-owned,
  eframe storage if persisted), Position card (coords, Y, heading in degrees,
  age, Freshness; inline Map Suggestion banner "Looks like Customs — Switch /
  Dismiss"; not-tracking card with **Choose folder…**).
- `chrome.rs`: File menu — Settings (screenshots folder: path field, Browse…,
  Use default), Clear Settings (in-place `ResetSettings`), Exit; attribution.
- `toasts.rs`: hand-rolled, draws `model.notifications()` bottom-right with
  action + close buttons; action-bearing Notifications are sticky, others time
  out by severity (Info 6 s / Warning 8 s / Error 10 s) → `DismissNotification`.
  `egui_toast` is dropped.
- `theme.rs`: colours/styles.
- Repaint: `request_repaint()` while `in_transition()`,
  `request_repaint_after(1 s)` while a position exists.
- eframe `persistence` keeps **window geometry only**; `App::save` no longer
  writes settings; no migration from the old `AppSettings` (one-off reset).
- Logging: every port error / runner failure is logged once at the app edge
  with `{err:?}`.

---

## 9. `fetch_maps` (`tarkov-map-fetch`) — [ADR-0006](../adr/0006-fetch-maps-is-the-anti-corruption-layer.md) — and `bc7z`

Resolved by [Decide: fetch_maps as anti-corruption layer](https://github.com/teevik/tarkov-map/issues/53).

- The only code that knows tarkov.dev's shapes (`maps.json`,
  `json.tarkov.dev/regular/maps`, `maps_en`). Upstream shapes are fetch-private
  DTOs; builds domain `Map` literals, bakes the Projection (tiles → rotation ∘
  transform ÷ tile size; SVG → rotation ∘ box over `svgBounds ?? bounds` ∘
  `xMidYMid meet` letterbox), derives tile-map bounds, applies the order table,
  validates with `MapCatalog::try_new`, writes the Bundle atomically (temp +
  rename, last).
- Lib modules: `upstream` (DTOs + `fetch_all`), `convert` (pure
  `convert(Upstream, &OrderTable) -> Conversion { maps, warnings, images_needed }`
  incl. `bake_projection`, oracle-tested), `images` (SVG/tile → PNG; BC7 → bc7z
  behind `encode`), `bundle`. `main.rs` = `Args` + `run(args)`.
- Required upstream fields per Map: `normalizedName`, an `interactive` entry,
  `name`, `bounds`, `coordinateRotation`, and `svgPath` or
  (`tilePath`+`tileSize`+`transform`+`maxZoom`). `svgBounds` optional but read.
- Failure taxonomy for a Refresh: missing value → drop + warn; unknown enum
  variant → fail the run; required field missing on a table Map → fatal,
  nothing written; incomplete non-table group → skip + warn; complete new Map →
  auto-include, appended alphabetically, loud warning.
- One merge rule: every collection is the union over canonical + `altMaps`
  entries, deduped by rounded position + identity; labels canonical-only;
  2-decimal rounding. `MapImageKey` = verbatim `maps/<id>.bc7z`.
- CLI keeps `--force`, `--tile-zoom-offset` (2), `--convert-only`, 2× SVG
  render; PNG intermediates are a gitignored cache. Errors: eros
  `context`+`backtrace`, warnings summarised with counts, non-zero exit iff fatal.
- **`bc7z`** (`tarkov-map-bc7z`): the container format (magic/header/zstd),
  `decode` always, `encode` behind the feature (`intel_tex_2`); round-trip
  tests live with the format.

---

## 10. Error handling — [ADR-0001 (amended)](../adr/0001-error-handling-with-eros.md)

Resolved by [Decide: error-handling conventions with eros](https://github.com/teevik/tarkov-map/issues/43),
amended by the [core skeleton prototype](https://github.com/teevik/tarkov-map/issues/54);
research: eros API ([ticket](https://github.com/teevik/tarkov-map/issues/36)).

- **Core has no eros dependency.** Core error types are hand-rolled
  (`#[derive(Debug)]` + manual `Display` + `std::error::Error`), one per
  port/failure domain, variants carrying what the model needs to act; opaque
  cause only as `source: Box<dyn Error + Send + Sync>`. No `thiserror`, no
  catch-all `CoreError`, no `String` payloads (except `RestartFailed(String)`,
  which is display text from the app).
- Adapters/app/fetch use eros freely; `impl Port` maps with `.into_inner()` and
  **no `.context`**; `.context` only in `main` and `fetch_maps`. Features:
  adapters `default-features = false`; app + fetch enable `context` + `backtrace`.
- No `Effect::Log`; the model turns user-relevant errors into Notifications;
  logging once at the app edge. Bundled `maps.ron` failure is fatal; `expect`
  only for build-broken invariants. eros gotcha: `bail!`/`ensure!` do not format
  a bare literal.

---

## 11. Dependencies

Research: [`docs/research/dependency-audit-blessed-rs.md`](../research/dependency-audit-blessed-rs.md)
([ticket](https://github.com/teevik/tarkov-map/issues/38)), extended by later tickets.

| Change | Where |
|---|---|
| Drop `base64`, `egui_extras`, rust-embed `compression` | phase 1 |
| `ico` → `eframe::icon_data::from_png_bytes`; `winres` → `winresource` | phase 1 |
| `regex` → hand parser | phase 3 |
| `serde_with` → `skip_serializing_if` / plain derives | phase 2 |
| `thiserror` → hand-rolled core errors + `eros` outside core | phase 4a |
| `egui_toast` → hand-rolled `toasts` | phase 4a |
| Add `euclid` (+`serde`) in core; `futures`/`tokio-stream`, `tokio-util` (`CancellationToken`) for the runner | phase 2 / 4a |
| Add `tempfile` (dev, adapters) | phase 2 |
| Keep `dirs`, `log` + `env_logger`, `self_update`, `notify`, `ron`, `zstd`, `rust-embed`, `intel_tex_2` (fetch `encode` only) | — |
| fetch-only deps (clap, reqwest, resvg, indicatif, serde_json) leave the app crate | phase 2 |

---

## 12. Testing strategy

Resolved by [Decide: testing strategy for the core](https://github.com/teevik/tarkov-map/issues/52).

| Area | Location | Style |
|---|---|---|
| Model (`handle`, queries) and Effect Runner | `crates/core/src/model/tests/{selection,viewport,positions,suggestion,image,settings,notifications,updater}.rs`, `runner/tests.rs` — `#[cfg(test)]` submodules | model: plain `#[test]`, `now` by value, assert state + returned `Vec<Effect>`; runner: `#[tokio::test]` current-thread over `FakePorts` |
| Small pure units (`parse_screenshot_name`, `ViewFrame`, `MapCatalog::try_new`, projection helpers, bc7z) | inline `#[cfg(test)] mod tests` | example-based; parser + `ViewFrame` golden cases |
| Adapters against the real filesystem / embedded assets | `crates/adapters/tests/*.rs` | integration tests, `tempfile` |
| Fetch affine baking | inline in the fetch **lib** (not behind `encode`) | oracle vectors |

- **Oracle vectors**: one data file `crates/core/testdata/projection-oracle.ron`
  — `{ map, game: (x, z), fraction: (fx, fy) }` × 38 transcribed from
  `docs/research/tarkov-dev-coordinate-oracle.md` (upstream `d3dc9b8`, Leaflet
  1.9.4); absolute tolerance `1e-3` on image fraction, one constant. fetch
  `include_str!`s it for baking tests; adapters for the bundled-catalog test.
  **No characterization tests** of today's `coordinates.rs`.
- **Fixtures**: `core::testing::catalog()` — synthetic 3-map catalog (`alpha`
  identity-ish 0..100, `beta` 200..300, `gamma` overlapping `beta`);
  `at(secs) -> SystemTime`; `Harness { model, effects }` with `send(msg)`.
- **Model invariants** (the suite): unknown/None settings map → first · index
  out of range no-op · reselect no-op · identical/older position ignored ·
  `Started` clears positions · Trail cap 20, reduced to newest on switch ·
  suggestion only for exactly one candidate, muted until back inside,
  re-evaluated on switch · non-selected `ImageDecoded` dropped · exactly one
  `PersistSettings` per settings mutation · `SetScreenshotsDir` effect order ·
  `ResetSettings` restores + persists + rewatches · viewport clamps,
  anchor-preserving zoom, reset on switch · center-on-player without position
  no-op · freshness flips at 120 s · offered overlays hide empty kinds but never
  PlayerMarker/Trail · notification dedupe + once-per-session BC error ·
  `CheckForUpdate` iff enabled · `UpdateAvailable` raises one sticky action
  Notification · `InstallUpdate` outside `Available` no-op · install dismisses
  Available and raises Downloading · `UpdateFailed` → `Available` ·
  `RestartFailed` keeps `Installed` · `ViewFrame` fit / anchor zoom / inverse
  round-trip · `overlay_items()` visibility, draw order, Trail index, heading
  through the Projection.
- **Runner tests (six, no sleeps)**: decode happy path; decode failure; new
  `DecodeImage` cancels in-flight (`Semaphore`-gated `FakeDecoder`);
  `WatchScreenshots` forwards + `StopWatching` ends; `PersistSettings`
  coalesces latest-wins; `CheckForUpdate`/`InstallUpdate` map fake results to
  the four update messages. `wake` counted via `AtomicUsize`.
- **Adapter integration tests**: `FileSettingsStore` round-trip on a tempdir ·
  `EmbeddedImageDecoder` decodes every bundled image · bundled catalog parses,
  validates, every `MapImageKey` resolves, every oracle vector within tolerance
  · `ScreenshotWatcher` on a tempdir (pre-existing newest PNG first, then a
  created one) bounded by a 5 s timeout (`#[ignore]` rather than delete if
  flaky) · `DemoPositionSource` yields `Started(Demo)` then positions ·
  non-`.png`/unparsable skipped.
- **No proptest** initially (candidates: `ViewFrame` round-trip, parser
  round-trip, zoom clamp under random sequences). CI gate = #16.

---

## 13. Behaviour: ride-along features and explicit non-goals

Resolved by [Decide: which app improvements and new features ride along](https://github.com/teevik/tarkov-map/issues/42).

**In** (each shapes the model; UI in phase 5): **Trail** (last 20 fixes,
Overlay toggle, on by default, cleared on map switch) · **Map Suggestion**
(inline banner in the Position card when a fix sits outside the selected Map
and inside exactly one other; never auto-switch; dismiss mutes until back
inside) · **Freshness** in core (Live/Stale, 120 s, hard-coded) · `Ctrl+1..9`
map shortcuts with Ctrl-held badges · fixed map order in `maps.ron` · minimal
**Settings dialog** (screenshots folder only) + **Choose folder…** on the
not-tracking card · in-place **Clear Settings** · Position card shows heading
in degrees · images drawn at their own pixel aspect (un-stretches Labs /
Labyrinth / Icebreaker) · Reserve and Icebreaker markers corrected.

**Explicitly not**: follow / recentre-on-fix mode; graduated or configurable
staleness; user reordering/favourites of maps; `Ctrl+0`; new sidebar
sections; periodic update rechecks or a manual check button; floors/layers.

**Out of scope for this effort** (see the map): manual coordinate entry,
multi-player/squad positions, annotations/drawing, runtime tarkov.dev
fetching, localisation, a headless CLI binary, the richer-overlays feature set
itself (owned by [#26](https://github.com/teevik/tarkov-map/issues/26)).

---

## Appendix A — Migration order

Resolved by [Decide: migration order appendix](https://github.com/teevik/tarkov-map/issues/55).
An order with named PR-sized slices and done-when criteria, not a backlog;
`/to-tickets` writes ticket bodies from this spec.

### Baseline and rules

- **Baseline = `main` after the richer-overlays implementation ([#70](https://github.com/teevik/tarkov-map/issues/70)) lands.**
  Overlay tickets finish on today's structure; the migration carries their
  types into the core `Map`.
- **Strict strangler.** Every slice is one PR merged to `main` that builds,
  passes `cargo test --workspace`, and leaves the app fully usable;
  release-please may cut a release from any point. No long-lived branch.
  Commits are `refactor:` unless behaviour changes (`fix:` for the
  Reserve/Icebreaker correction, `feat:` for phase 5).
- **Old code is deleted in the slice that replaces it**, never only in a final sweep.
- **Each phase carries a 4–6 item manual parity checklist** ticked in the PR:
  all Maps render, markers place correctly (spot-check the 7 oracle maps), demo
  mode runs, screenshot tracking works, update check/notify works, settings
  survive a restart.

### Phases

1. **Workspace + bc7z + hygiene.** Add `[workspace]`, extract `crates/bc7z`
   (`encode` feature) and point the old app and `fetch_maps` at it. Separate
   tiny PR: structure-independent dependency swaps — drop `base64`,
   `egui_extras`, `ico` (→ `eframe::icon_data::from_png_bytes`), rust-embed
   `compression`; `winres` → `winresource`. *Done when:* workspace builds,
   release artefact unchanged.
2. **Core domain + fetch + Bundle.** `crates/core` domain types (`Map`,
   `MapCatalog::try_new`, euclid geometry, `PlayerPosition`),
   `crates/fetch-maps` rewritten as lib + bin per ADR-0006, `maps.ron`
   regenerated in the new schema (one Refresh), `crates/adapters` with the
   bundled-catalog loader + `EmbeddedImageDecoder`; old app switches to the new
   catalog and projects through the baked affine; delete `src/lib.rs` `Map`,
   `catalog.rs`, `coordinates.rs`; `serde_with` goes. Port the prototype's
   domain modules. **Reserve and Icebreaker markers visibly move (intended).**
   *Done when:* oracle tests pass in fetch and adapters, old UI renders every
   Map from the new Bundle.
3. **Position Source.** `parse_screenshot_name` + golden vectors in core,
   `ScreenshotWatcher` / `DemoPositionSource` / `AnyPositionSource` in
   adapters; old app consumes the stream; `regex` and the old watcher/demo
   deleted. *Done when:* tracking and demo work via the adapters.
4. **Model + Effect Runner + app cutover** — two PRs:
   - **4a** `Model`/`Msg`/`Effect`/Runner in core (ported from
     `prototype/core-skeleton`; this spec is authority where they differ),
     **full model as specified** — Trail deque, suggestion, update state,
     shortcuts — even though the UI shows none of it yet; remaining ports +
     adapters (`FileSettingsStore` → `settings.ron`, `GithubReleaseFeed`); the
     old UI translated to `dispatch(Msg)` with app-side state deleted; updater
     model-owned; hand-rolled toasts (drop `egui_toast`, `thiserror`).
     Parity-preserving; no new features visible.
   - **4b** Mechanical reorganisation of the app into the ADR-0005 module
     layout (`app/input/map_view/sidebar/chrome/textures/toasts/theme`),
     `src/bin/tarkov-map/` → `src/`.
   *Done when:* no state lives outside `Model` except presentation state named
   in ADR-0005.
5. **Ride-along features** — one `feat:` ticket each, UI + adapter work only
   since the model already has them: Trail overlay, Map Suggestion banner,
   `Ctrl+1..9` + held-Ctrl badges, settings dialog + Choose-folder on the
   not-tracking card, in-place Clear Settings, staleness bands. Parallelisable.
6. **Sweep.** Delete leftovers, remaining dependency swaps, unused-dependency
   check, README / `docs/agents` / `CLAUDE.md` updated to the workspace;
   `prototype/core-skeleton` and `research/*` branches deleted (their content
   is now in the repo: research notes under `docs/research/`, the prototype
   ported in phase 4a).

---

## Appendix B — Index

**ADRs** (`docs/adr/`)

| ADR | Decision |
|---|---|
| [0001](../adr/0001-error-handling-with-eros.md) | Error handling: hand-rolled core errors, eros outside core, boundary-only context, no eros in core (amended) |
| [0002](../adr/0002-sync-model-effects-out-async-in-one-runner.md) | Sync model, effects out, async confined to one Effect Runner; app intercepts only `PresentImage`/`Restart` |
| [0003](../adr/0003-projection-is-a-baked-affine-per-map.md) | Projection is one pre-baked game→image affine per Map, decided by fetch_maps |
| [0004](../adr/0004-cargo-workspace-with-the-app-as-root-package.md) | Cargo workspace with the egui app as the root package; fetch is lib + bin (amended) |
| [0005](../adr/0005-core-yields-image-space-items-app-paints.md) | Rendering boundary: core yields Image-space items, the app alone touches the screen |
| [0006](../adr/0006-fetch-maps-is-the-anti-corruption-layer.md) | fetch_maps is the anti-corruption layer: one validator, fail loud on contract drift |

**Research notes** (`docs/research/`)

- [eros error-handling crate](https://github.com/teevik/tarkov-map/issues/36) — findings in the ticket (no separate note)
- [`geometry-crate-for-core.md`](../research/geometry-crate-for-core.md) — euclid vs glam vs hand-rolled
- [`dependency-audit-blessed-rs.md`](../research/dependency-audit-blessed-rs.md)
- [`tarkov-dev-coordinate-oracle.md`](../research/tarkov-dev-coordinate-oracle.md) — the 38 golden vectors
- [`async-ports-and-effects.md`](../research/async-ports-and-effects.md)
- [`workspace-mechanics.md`](../research/workspace-mechanics.md)

**Decision tickets** (resolution comments hold the full record):
[ride-along features #42](https://github.com/teevik/tarkov-map/issues/42) ·
[error conventions #43](https://github.com/teevik/tarkov-map/issues/43) ·
[Player Position #44](https://github.com/teevik/tarkov-map/issues/44) ·
[async shape #45](https://github.com/teevik/tarkov-map/issues/45) ·
[domain Map #46](https://github.com/teevik/tarkov-map/issues/46) ·
[application model #47](https://github.com/teevik/tarkov-map/issues/47) ·
[ports catalogue #48](https://github.com/teevik/tarkov-map/issues/48) ·
[workspace layout #49](https://github.com/teevik/tarkov-map/issues/49) ·
[rendering boundary #50](https://github.com/teevik/tarkov-map/issues/50) ·
[updater/settings/notifications #51](https://github.com/teevik/tarkov-map/issues/51) ·
[testing strategy #52](https://github.com/teevik/tarkov-map/issues/52) ·
[fetch_maps ACL #53](https://github.com/teevik/tarkov-map/issues/53) ·
[core skeleton prototype #54](https://github.com/teevik/tarkov-map/issues/54) ·
[migration order #55](https://github.com/teevik/tarkov-map/issues/55).

**Reference implementation**: branch
[`prototype/core-skeleton`](https://github.com/teevik/tarkov-map/tree/prototype/core-skeleton)
(`crates/core`, 16 tests, `crates/core/PROTOTYPE.md`) — port in phase 4a;
this spec wins where they differ.
