# Dependency audit against blessed.rs

Research for wayfinder ticket #38 (parent map issue #35). Question: audit every
dependency in `Cargo.toml` against blessed.rs and mainstream practice, and
propose concrete swaps/drops/additions that materially simplify the crate. Each
row is meant to be accepted or rejected on its own.

## Sources

All findings are from primary sources, read on 2026-08-17:

- `Cargo.toml` / `Cargo.lock` at `origin/main` (0.1.14), plus `cargo tree -e
  normal` / `cargo tree -i <crate>` for subtree sizes and "who else pulls this".
  Usage was established by grepping `src/` (see "Actual usage" below).
- blessed.rs crate directory — <https://blessed.rs/crates>. Notable: it lists
  `regex`, `tempfile`, `reqwest`, `ureq`, `thiserror`, `anyhow`, `tracing`
  ("now the go-to crate for logging"), `log` ("older and simpler crate if your
  needs are simple and you are not using any async code"), `serde`,
  `serde_json`, `toml`, `tokio`, `clap`, `notify`, `dirs`, `directories`
  ("a higher-level library that can also compute paths for applications"),
  `etcetera` ("an alternative with a different license"), `indicatif`,
  `crossbeam-channel`, `flume`, `insta`, `criterion`, `egui`. It has **no
  entry** for `ron`, `serde_with`, `image`, `resvg`, `zstd`, `rust-embed`,
  `self_update`, `open`, `base64`, `ico`, `env_logger`, `winres`, parser
  combinators (`nom`/`winnow`), or `eros`.
- crates.io API (`/api/v1/crates/<name>` and `/versions`) for latest version and
  release dates: `dirs` 6.0.0 (2025-01-12), `directories` 6.0.0 (2025-01-12),
  `self_update` 0.44.0 (2026-04-05; 1.0.0-rc.1..rc.6 released 2026-06-22 ..
  2026-07-16), `egui-toast` 0.22.0 (2026-08-08), `winres` 0.1.12
  (2021-09-28, last release), `winresource` 0.1.31 (2026-03-16), `ico` 0.5.0
  (2025-11-28), `ron` 0.12.2 (2026-06-22), `open` 5.4.1 (2026-08-05), `notify`
  8.x stable / 9.0.0-rc.4 (2026-05-02), `rust-embed` 8.12.0 (2026-07-08).
- `dirs-dev/dirs-rs` GitHub repo — archived on GitHub 2025-02-18, "moved to
  <https://codeberg.org/dirs/dirs-rs>" (still maintained there; v6 is current).
- `BenjaminRi/winresource` README — "`winresource` is a fork of `winres` ...
  [winres] no longer works on Rust 1.61 or higher and has been left
  unmaintained (mxre/winres#40)"; same `WindowsResource::new().set_icon(..).compile()` API.
- `jaemk/self_update` README — GitHub backend, `reqwest`+`rustls` default,
  SHA-256 digest verification against GitHub release checksums, optional
  zipsign signatures.
- docs.rs `etcetera` 0.11.0 — exposes config/data/cache/state/runtime dirs and
  `home_dir()` only; **no documents dir**.
- docs.rs `eros` 0.7.0 (2026-07-16, Apache-2.0) — `ErrorUnion<(A, B)>`,
  `.context()`, `#[context]`, `bail!/ensure!`, backtrace/location capture.
- `SOF3/include-flate` `src/lib.rs` (used by `rust-embed`'s `compression`
  feature) — `flate!` expands to a `static LazyLock<Vec<u8>>`, i.e. the
  decompressed bytes stay resident after first access; supports deflate and
  zstd. `rust-embed-impl` 8.12.0 `src/lib.rs:346` emits `flate!(static BYTES:
  IFlate from #path with #compression_ident)` per embedded file.
- eframe 0.36.1 `src/icon_data.rs` — `eframe::icon_data::from_png_bytes(&[u8])
  -> Result<IconData, image::ImageError>` is always available (`image` is a
  non-optional eframe dep, `png` feature).

## TL;DR

Ordered by payoff / cost:

1. **Drop `base64` and `egui_extras`** — neither is referenced anywhere in
   `src/` (they were last used in commits `81f40ec` / `6b47353`). `egui_extras`
   with `svg` also drags a second `resvg`/`usvg` 0.45 tree next to our 0.48.
2. **Gate the `fetch_maps`-only deps behind the existing bin feature**
   (`clap`, `indicatif`, `resvg`, `image`, `tokio`, `reqwest`, `serde_json`,
   `intel_tex_2`) so an app build stops compiling clap/resvg/tokio-`full`.
3. **`regex` → hand parse** the fixed `_x, y, z_qx, qy, qz, qw_` filename
   (`split('_')` + `split(", ")`); also removes the per-call `Regex::new`.
4. **`serde_with` → `#[serde(skip_serializing_if = "Option::is_none")]`** on
   the 14 `Option` fields in `src/lib.rs` (proc-macro subtree of 16 crates for
   one attribute).
5. **`winres` → `winresource`** (unmaintained since 2021 vs maintained fork,
   drop-in).
6. **`rust-embed`: drop the `compression` feature** — `.bc7z` is already zstd;
   double compression buys nothing and pins decompressed bytes in a `LazyLock`.
7. **`ico` → `eframe::icon_data::from_png_bytes`** with a PNG icon (zero new
   deps; `image` is already in eframe).
8. **Keep** `dirs` (only crate in the set with `document_dir`; `etcetera` lacks
   it, `directories` is the same authors/cadence), **keep** `log`+`env_logger`
   (blessed.rs's own guidance for simple non-async apps; egui/eframe/winit log
   via `log`), **keep** `self_update`, `notify`, `ron`, `zstd`, `open`,
   `egui-toast`, `serde`, `eframe`.
9. `thiserror` → `eros` is already decided elsewhere; noted for completeness
   (`thiserror` 2 stays in the tree via wgpu regardless).
10. Test helpers: nothing needed today. Add `tempfile` (blessed.rs) as a
    dev-dep only when writing filesystem tests for the screenshot watcher; skip
    `insta`/`proptest`.

## Actual usage (from `src/`)

| Crate | Where | What for |
| --- | --- | --- |
| `eframe`, `egui` | app | UI, wgpu backend, persistence |
| `egui-toast` | `updater.rs`, `main.rs` | update/error toasts |
| `egui_extras` | — | **unused** (was `install_image_loaders`, removed in `6b47353`) |
| `base64` | — | **unused** (removed in `81f40ec`) |
| `ico` | `main.rs::load_icon` | decode entry `[2]` of `assets/tarkov-map-icon.ico` into `egui::IconData` |
| `image` | `fetch_maps.rs` | PNG open/overlay for BC7 encoding |
| `resvg` | `fetch_maps.rs` | render map SVG to pixmap |
| `indicatif`, `clap` | `fetch_maps.rs` | progress bars, CLI args |
| `reqwest`, `tokio` (`full`), `serde_json` | `fetch_maps.rs` | async downloads (`JoinSet`, `Semaphore`, `tokio::fs`), JSON API |
| `intel_tex_2` | `fetch_maps.rs` (feature `encode`) | BC7 encoding |
| `zstd` | `src/bc7z.rs` | `encode_all(.., 19)` (fetch) / `decode_all` (app) |
| `ron` | `assets.rs`, `fetch_maps.rs` | parse/write `maps.ron` |
| `serde`, `serde_with` | `src/lib.rs` | data model; `#[skip_serializing_none]` on 3 structs |
| `rust-embed` (`compression`, `include-exclude`) | `assets.rs` | embed `assets/` (`.bc7z`, `maps.ron`), fs in debug |
| `notify` | `screenshot_watcher.rs` | watch Screenshots dir for `Create` events |
| `regex` | `screenshot_watcher.rs::parse_screenshot_filename` | extract 7 floats from filename; `Regex::new` runs on every call |
| `dirs` | `screenshot_watcher.rs::screenshots_path` | `dirs::document_dir()` → `Escape from Tarkov/Screenshots` |
| `log`, `env_logger` | app + fetch | `log::{info,warn,error,debug}!`, `env_logger::init()` |
| `thiserror` | `assets.rs`, `bc7z.rs`, `fetch_maps.rs` | error enums |
| `self_update` | `updater.rs` | GitHub release check + install (`bump_is_compatible`) |
| `open` | `ui.rs` | `open::that(repo url)` |
| `winres` | `build.rs` (windows) | exe icon |
| std only | `main.rs`, `updater.rs`, `assets.rs` | `std::thread::spawn` + `std::sync::mpsc` for background work polled from the egui frame loop |

Tree size at `origin/main`: 554 unique packages (`cargo tree -e normal`).
Sub-trees: `self_update` 162, `reqwest` 139, `egui_extras` 122, `egui-toast`
76 (mostly shared egui), `rust-embed` 59, `image` 27, `tokio` 26, `clap` 19,
`env_logger` 18, `serde_with` 16, `ron` 14, `ico` 13, `notify` 12, `regex` 6,
`indicatif` 6, `open` 5, `dirs` 4, `zstd` 3.

## Decision table

| Dependency | Used for | blessed.rs | Recommendation | Why | Migration cost |
| --- | --- | --- | --- | --- | --- |
| `base64` 0.23 | nothing | not listed | **drop** | No reference in `src/`; the `base64` 0.22 in the lock comes from reqwest/hyper-util/usvg and is unaffected. | Delete one line. |
| `egui_extras` 0.36 (`svg`,`image`) | nothing | not listed (egui is) | **drop** | No reference in `src/`. Its `svg` feature pulls a duplicate `resvg`/`usvg`/`tiny-skia` 0.45 tree beside our `resvg` 0.48. | Delete one line; `cargo build` to confirm. |
| `regex` 1.13 | parse `_x, y, z_qx, qy, qz, qw_` out of the screenshot filename | listed ("de facto standard") | **swap → hand parse** (no crate) | Format is fixed and delimiter-based: `filename.split('_')` → segments 1 and 2, `split(", ")` → 3 and 4 floats, `parse::<f64>()`. ~15 lines, unit-testable without files, and it removes the `Regex::new` that currently recompiles on every parse. `winnow`/`nom` are overkill for one delimiter split. Note `regex` stays in the lock anyway (via `env_filter` and `self_update`), so the win is code clarity, not build time. | Small: rewrite one fn, add 2-3 tests with real filenames from the doc comment. |
| `dirs` 6.0 | `document_dir()` | listed | **keep** | Only candidate that exposes Documents: `etcetera` has config/data/cache/state/runtime/home only; `directories` (`UserDirs::document_dir`) is the same maintainer, same release date, bigger API. Repo moved GitHub → Codeberg (Feb 2025) but is not abandoned; v6 released Jan 2025; 4-crate subtree. Documents can be relocated on Windows (OneDrive), so `home_dir()/Documents` is not a safe replacement. | none. If you want to migrate anyway: `directories::UserDirs::new()?.document_dir()?` — same cost, no benefit. |
| `log` 0.4 + `env_logger` 0.11 | app/fetch logging via `RUST_LOG` | `log`: "older and simpler crate if your needs are simple and you are not using any async code"; `tracing`: "the go-to" | **keep** | The app is synchronous (`std::thread` + `mpsc`), has no spans, and eframe/egui/winit/wgpu emit via `log`. `tracing` would need `tracing-subscriber` (+`env-filter`, which itself pulls `regex`/`matchers`) and `tracing-log` to bridge egui's `log` records — strictly more crates for the same stderr output. Revisit only if structured/spanned logs are wanted. | none (swap would be ~5 lines + 3 crates). |
| `thiserror` 2.0 | error enums | listed | **swap → `eros`** (already decided) | Recorded for completeness. `eros` 0.7.0 is pre-1.0 (Apache-2.0); `thiserror` 2 remains in the tree via wgpu/naga either way. | Per the existing decision. |
| `serde_with` 3.21 | `#[skip_serializing_none]` ×3 | not listed | **swap → plain serde attrs** | One attribute is the whole usage; `#[serde(skip_serializing_if = "Option::is_none")]` on the 14 `Option` fields is stock serde and drops a 16-crate proc-macro subtree (darling/syn). Only affects fetch_maps writing `maps.ron`; deserialisation is unchanged. | Small: 14 attribute lines, remove 3 macros + 1 import; regenerate/diff `maps.ron` once. |
| `serde`, `serde_json`, `ron` | data model, API JSON, `maps.ron` | serde/serde_json listed; ron not | **keep** (`serde_json` → gate, see below) | Standard. `ron` 0.12.2 released 2026-06; asset format is fine and human-diffable. | none. |
| `tokio` (`full`) | fetch_maps async | listed | **keep, trim + gate** | App code is sync; only fetch_maps needs a runtime. `full` enables process/signal/net/io-std for nothing; `rt-multi-thread`, `macros`, `fs`, `sync` cover `#[tokio::main]`, `JoinSet`, `Semaphore`, `tokio::fs`. Note `reqwest` (via `self_update`) keeps a base tokio in the app tree regardless. No `tokio-util`/`flume`/`crossbeam-channel` needed: the frame-loop `try_recv` pattern is exactly what `std::sync::mpsc` does; blessed.rs lists the alternatives as options, not upgrades. | Small: feature list edit + `optional = true`. |
| `reqwest`, `clap`, `indicatif`, `resvg`, `image`, `intel_tex_2` | fetch_maps only | reqwest/clap/indicatif listed | **keep, gate behind the fetch bin feature** | `fetch_maps` already has `required-features = ["encode"]`; make the rest `optional = true` and add them to that feature (rename to `fetch` if wanted). App builds stop compiling `clap`, `resvg` 0.48, `image`, tokio-`full` (`reqwest`/`indicatif`/`serde_json` are still pulled by `self_update`, so no loss there). | Small: Cargo.toml only; no code change. |
| `zstd` 0.13 | bc7z encode (19) / decode | not listed | **keep** | C `zstd-sys` builds fine on the MSVC CI target; encoding at level 19 needs the real library. A pure-Rust `ruzstd` decoder for the app alone would split the codec across two crates for little gain. | none. |
| `rust-embed` 8.12 (`compression`, `include-exclude`) | embed `assets/` | not listed | **keep, drop `compression`** | Payload is already zstd (`.bc7z`); `include-flate` re-compresses it at build time and, per its `flate!` expansion, keeps the decompressed copy in a `static LazyLock<Vec<u8>>` for the process lifetime — the opposite of the app's "free inactive textures" retention policy. Dropping the feature also removes `include-flate*` + a second `zstd` edge. Keep `include-exclude` (used for `maps/*.png`) and the debug-mode fs loading. | Trivial: feature list edit. |
| `notify` 8.0 | watch Screenshots dir | listed | **keep** | Standard; 8.x is the current stable line (9.0.0-rc.4 as of May 2026). Optional: pin the callback to `EventKind::Create` only as today. | none. |
| `self_update` 0.44 | GitHub release check/install | not listed | **keep** | Does the whole feature (target asset selection, SHA-256 digest check, self-replace) that would otherwise be ~300 lines of reqwest + semver + self-replace. Heavy (162 pkgs, mostly reqwest/hyper/rustls) but that stack is needed for any updater. 1.0.0-rc.6 is out (2026-07-16); wait for 1.0.0 stable before bumping. | none now; expect API churn at 1.0. |
| `egui-toast` 0.22 | toasts | not listed | **keep** | Maintained (0.22.0 on 2026-08-08, tracks egui 0.36); `egui-notify` is the only peer and offers nothing extra here. | none. |
| `open` 5.4 | open repo URL | not listed | **keep** | Tiny (5 pkgs), released 2026-08-05, standard for "open in browser". | none. |
| `ico` 0.5 | decode `.ico` for the window icon | not listed | **swap → `eframe::icon_data::from_png_bytes`** | eframe already depends on `image` (`png`) and exposes this helper unconditionally; ship a 256px (or the size currently at entry `[2]`) PNG next to the `.ico` (the `.ico` is still needed by `winres`). Removes `ico` and its own `png` decoder path from the app; `load_icon()` becomes one line. | Small: export one PNG from the existing `.ico`, 3-line code change. |
| `winres` 0.1.12 | exe icon on Windows | not listed | **swap → `winresource`** | `winres` last released 2021-09; `winresource` README states it forked because `winres` "has been left unmaintained" and doesn't work on Rust ≥1.61 (our MSVC CI happens to still work). Same `WindowsResource::new().set_icon().compile()` API; 0.1.31 released 2026-03-16. | Trivial: rename in Cargo.toml and `build.rs`. |
| `eframe` 0.36 (wgpu, wayland, x11, persistence, accesskit, links) | the app | `egui` listed | **keep** | Fine. `links` feature is what `open`-style hyperlinks in egui need; unrelated to the `open` crate use in `ui.rs`. | none. |
| test helpers (`tempfile`, `insta`, `proptest`) | none today | `tempfile`, `insta` listed | **add nothing now**; `tempfile` as dev-dep only when needed | Existing tests (`bc7z.rs`, `assets.rs`) are pure and use std channels as fakes, which is fine. If the hand-parsed filename parser is split from the fs metadata lookup, its tests need no temp files either. `tempfile` is the blessed choice the day watcher/end-to-end fs tests are written; snapshot/property testing has no target in this codebase. | none. |

## Notes on rejected candidates

- `directories` / `etcetera` for `dirs`: `etcetera` cannot answer the question
  the app asks (Documents folder). `directories` can, but it is the same
  author, same version number and release date as `dirs`, so it is a lateral
  move with a larger API surface.
- `tracing` for `log`: blessed.rs's own qualifier ("if your needs are simple and
  you are not using any async code") describes this app exactly; the async
  part (fetch_maps) is a one-shot CLI. `tracing` 0.1 is already in the lock via
  winit/h2/zbus, but the subscriber and the `log` bridge are the real cost.
- `winnow`/`nom` for `regex`: a single fixed-delimiter line does not need a
  combinator library; the std `split` version is shorter than either.
- `crossbeam-channel` / `flume` / `tokio-util` for the app's channels: nothing
  in `assets.rs`/`updater.rs`/`screenshot_watcher.rs` uses `select!`, bounded
  back-pressure or async receivers; `std::sync::mpsc` (which since Rust 1.67
  is the crossbeam implementation) already covers try-recv polling from the
  frame loop.
- `ureq` for `reqwest`: `self_update` requires `reqwest` (blocking), so a
  second HTTP client would only add crates.

## Suggested Cargo.toml shape (if all accepted)

```toml
[dependencies]
eframe = { ... }                      # unchanged
egui-toast = "0.22"
serde = { version = "1", features = ["derive"] }
ron = "0.12"
rust-embed = { version = "8", features = ["include-exclude"] }
notify = "8"
dirs = "6"
log = "0.4"
env_logger = { version = "0.11", features = ["auto-color", "humantime"] }
eros = "0.7"                          # per the separate decision
self_update = { version = "0.44", default-features = false, features = ["reqwest", "rustls"] }
open = "5"
zstd = "0.13"

# fetch_maps only
clap = { version = "4", features = ["derive"], optional = true }
indicatif = { version = "0.18", optional = true }
resvg = { version = "0.48", optional = true }
image = { version = "0.25", default-features = false, features = ["png"], optional = true }
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls", "webpki-roots", "http2"], optional = true }
serde_json = { version = "1", optional = true }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "sync"], optional = true }
intel_tex_2 = { version = "0.4", optional = true }

[target.'cfg(windows)'.build-dependencies]
winresource = "0.1"

[features]
fetch = ["dep:clap", "dep:indicatif", "dep:resvg", "dep:image", "dep:reqwest",
         "dep:serde_json", "dep:tokio", "dep:intel_tex_2"]

[[bin]]
name = "fetch_maps"
required-features = ["fetch"]
```

Removed outright: `base64`, `egui_extras`, `regex`, `serde_with`, `ico`,
`winres`, `thiserror` (→ `eros`).
