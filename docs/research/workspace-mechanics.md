# Cargo workspace split: mechanics and release impact

Research for wayfinder ticket #41 (parent map issue #35). Question: what are
the mechanical consequences of splitting the single `tarkov-map` crate into a
Cargo workspace (core / adapters / egui app, `fetch_maps` placement TBD): how
to isolate the `encode` feature and `intel_tex_2` (prebuilt ISPC/C++ kernel),
how `rust-embed` resolves `folder` across crates, where `build.rs`/winres
belong, and what changes in `.github/workflows/{ci,release-please,release}.yml`,
`.github/release-please-*.json`, and `nix/`/`flake.nix`.

## Sources

All findings are from primary sources, read on 2026-08-17:

- The Cargo Book (stable):
  - Workspaces — <https://doc.rust-lang.org/cargo/reference/workspaces.html>
  - Specifying dependencies (inheriting from a workspace) —
    <https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html>
  - Features (resolver v2 feature flags, `required-features`) —
    <https://doc.rust-lang.org/cargo/reference/features.html>,
    <https://doc.rust-lang.org/cargo/reference/cargo-targets.html>
  - Build scripts — <https://doc.rust-lang.org/cargo/reference/build-scripts.html>
  - `cargo build` man page (`--locked`, `--workspace`, `-p`, `--features`) —
    <https://doc.rust-lang.org/cargo/commands/cargo-build.html>
- rust-embed 8.12.0 (the version in `Cargo.lock`): docs.rs README
  <https://docs.rs/crate/rust-embed/latest> and the derive source in the local
  registry, `rust-embed-impl-8.12.0/src/lib.rs`; include-flate-codegen 0.3.4
  `src/lib.rs` (used by rust-embed's `compression` feature).
- intel_tex_2 0.4.0 `build.rs` (local registry).
- release-please (googleapis/release-please, `main` on 2026-08-17):
  - `README.md` (Rust strategy row), `docs/manifest-releaser.md`
  - `src/strategies/rust.ts`, `src/strategies/base.ts` (`addPath`),
    `src/updaters/rust/cargo-toml.ts`, `src/plugins/cargo-workspace.ts`,
    `src/plugins/workspace.ts`, `src/plugins/linked-versions.ts`,
    `src/util/commit-split.ts`
  - Issue #1998 "Fails on cargo workspace" and its fix PR #2001 (docs only) —
    <https://github.com/googleapis/release-please/issues/1998>
- nixpkgs manual, Rust section
  (`doc/languages-frameworks/rust.section.md` on `master`).
- This repo at `8fcba7a` (release 0.1.14): `Cargo.toml`, `build.rs`,
  `src/bin/tarkov-map/assets.rs`, `src/bin/fetch_maps.rs`, `src/bc7z.rs`,
  `.github/workflows/*.yml`, `.github/release-please-config.json`,
  `.github/release-please-manifest.json`, `flake.nix`, `nix/devshell.nix`.

cargo-dist is not used by this repo; releases are built by
`.github/workflows/release.yml` (`cargo build --release` + `gh release upload`).

## TL;DR

- **Layout recommendation: keep the egui app as the workspace *root package*
  (`Cargo.toml` at repo root keeps `[package] name = "tarkov-map"` and gains
  `[workspace] members = ["crates/core", "crates/adapters", ...]`).** This is a
  first-class Cargo layout ("root package") and is the only layout under which
  the existing release-please config (`"."`, `release-type: rust`,
  `include-component-in-tag: false`), the root `CHANGELOG.md`, the `vX.Y.Z`
  tags, `release.yml`, and `rust-embed`'s `#[folder = "assets/"]` all keep
  working with **zero or near-zero edits**. A *virtual* manifest at the root
  breaks release-please's Rust strategy for the `"."` package outright
  (`is not a package manifest (might be a cargo workspace)`), and moving the
  app to `crates/app` splits the changelog by path (commits touching only
  `crates/core` would no longer appear in — or trigger — an app release
  without the `linked-versions` plugin, which then writes a synthetic
  "Synchronize versions" note instead of the real feature list).
- **`intel_tex_2`/`encode`**: keep it as an optional feature gating a
  `required-features` binary; whichever crate hosts `fetch_maps` (the root app
  crate, or a `crates/tools` crate). If a separate `tools` crate takes
  `intel_tex_2` as a *non-optional* dependency, `cargo build --workspace`,
  `cargo test --workspace`, and — in a virtual workspace without
  `default-members` — plain `cargo build` at the root all link the prebuilt
  C++ static libraries. `required-features` keeps `fetch_maps` skipped unless
  `--features encode` (or `-p tools --features encode`) is passed, exactly as
  today.
- **rust-embed**: `folder` is resolved against `CARGO_MANIFEST_DIR` of the
  crate containing the `#[derive(RustEmbed)]` — at *macro-expansion time*, in
  both debug (dynamic filesystem loading) and release (embedded) modes; the
  README's "relative to where the binary is run from" for debug is not what
  the 8.12.0 code does (the absolute path is baked into the generated `get()`).
  With the `compression` feature (enabled here) `folder` **must** be relative.
  So `assets/` must be reachable by a relative path from the app crate's
  manifest: unchanged if the app stays the root package; `../../assets/` if the
  app moves to `crates/app`. `fetch_maps` writes to `env!("CARGO_MANIFEST_DIR")/assets`
  and needs the same treatment.
- **build.rs / winres**: build scripts and `[build-dependencies]` are
  per-package; the script's cwd is the package root, so `build.rs` and
  `[target.'cfg(windows)'.build-dependencies] winres` move with the app crate
  and `set_icon("assets/tarkov-map-icon.ico")` stays valid only if the app is
  the root package (else `../../assets/...`).
- **`[workspace.package]` / `[workspace.dependencies]`**: use them for
  `edition`, `license`, and shared dep versions. **Do not** use
  `version.workspace = true` in any crate release-please touches: the Rust
  strategy's `CargoToml` updater does `replaceTomlValue(["package","version"])`
  and never updates `[workspace.package].version`; the `cargo-workspace`
  plugin throws `has an invalid [package.version]` when `version` is a table.
- **CI/release edits** (root-package layout): `ci.yml` should add
  `cargo test --workspace` (today CI only builds); `release.yml`'s
  `cargo build --release` at the root builds the root package (the app) — the
  artifact path `target/release/tarkov-map.exe` is unchanged because the
  workspace shares one `target/`; safer to write `cargo build --release -p
  tarkov-map` explicitly. release-please config/manifest: **no change**.
  Nix: there is no package derivation yet (only `nix/devshell.nix`); a future
  `buildRustPackage` needs `cargoLock.lockFile = ./Cargo.lock` at the root and
  `cargoBuildFlags = ["-p" "tarkov-map"]` (or `buildAndTestSubdir` for a
  non-root app crate).

## (a) Workspace layout options and inheritance

Two layouts (Cargo book, "Workspaces"):

- **Root package**: "If the `[workspace]` section is added to a `Cargo.toml`
  that already defines a `[package]`, the package is the *root package* of the
  workspace."
- **Virtual manifest**: "a `Cargo.toml` file can be created with a
  `[workspace]` section but without a `[package]` section... typically useful
  when there isn't a 'primary' package". "`resolver` must be set explicitly in
  virtual workspaces as they have no `package.edition` to infer it from" (the
  book's example uses `resolver = "3"` with edition 2024 members and notes the
  member `edition ... will have no effect on a resolver used in the
  workspace").

Members: "All `path` dependencies residing in the workspace directory
automatically become members. Additional members can be listed with the
`members` key"; globs (`crates/*`) are supported. Note release-please's Rust
strategy reads `workspace.members` *literally* and does not expand globs (see
(e)) — the `cargo-workspace` plugin does.

Package selection at the root: "if the current directory is a workspace root,
the `default-members` will be used... When unspecified, the root package will
be used. In the case of a virtual workspace, all members will be used (as if
`--workspace` were specified)". "Note: when a root package is present, you can
only operate on it using `--package` and `--workspace` flags" (i.e. once you set
`default-members`, the root package is addressed via `-p`/`--workspace`).
Consequence: with the root-package layout, `cargo build`/`cargo run`/
`cargo test` at the root operate on the app only; `cargo test --workspace`
runs core/adapters tests. `Cargo.lock` and `target/` are shared: "All packages
share a common `Cargo.lock` file which resides in the workspace root" / "a
common output directory, which defaults to a directory named `target` in the
workspace root".

`[workspace.package]` inheritable fields: `authors, categories, description,
documentation, edition, exclude, homepage, include, keywords, license,
license-file, publish, readme, repository, rust-version, version`; a member
opts in per field (`edition.workspace = true`).

`[workspace.dependencies]`: members write `dep = { workspace = true, ... }`.
"Along with the `workspace` key, dependencies can also include these keys:
`optional` (note that the `[workspace.dependencies]` table is not allowed to
specify `optional`), `features` (additive with the features declared in
`[workspace.dependencies]`). Other than `optional` and `features`, inherited
dependencies cannot use any other dependency key (such as `version` or
`default-features`)." So `intel_tex_2 = { workspace = true, optional = true }`
is legal; `default-features = false` must be stated once, in the workspace
table (e.g. for `eframe`, `image`, `reqwest`, `self_update`).

Sketch (placeholder names):

```toml
# Cargo.toml (repo root) — root package = the egui app
[workspace]
members = ["crates/core", "crates/adapters"]   # + "crates/tools" if split out
resolver = "3"                                # optional here; edition 2024 implies it

[workspace.package]
edition = "2024"
license = "MIT"

[workspace.dependencies]
tarkov-map-core = { path = "crates/core" }    # placeholder names
tarkov-map-adapters = { path = "crates/adapters" }
serde = { version = "1", features = ["derive"] }
thiserror = "2"
zstd = "0.13"
intel_tex_2 = "0.4"
# ...

[package]
name = "tarkov-map"
version = "0.1.14"           # literal — see (e)
edition.workspace = true
license.workspace = true
default-run = "tarkov-map"

[dependencies]
tarkov-map-core.workspace = true
tarkov-map-adapters.workspace = true
intel_tex_2 = { workspace = true, optional = true }
# ...

[features]
encode = ["dep:intel_tex_2"]

[[bin]]
name = "fetch_maps"
required-features = ["encode"]
```

## (b) Isolating `intel_tex_2` / the `encode` feature

What `intel_tex_2` 0.4.0 does at build time (its `build.rs`, non-`ispc`
default): `ispc_rt::PackagedModule::new("kernel").lib_path("src/ispc").link()`
plus `cargo:rustc-link-lib=static=ispc_texcomp_astc<TARGET>` and
`cargo:rustc-link-search=native=<crate>/src/ispc`. It ships prebuilt static
libraries per target (`kernelx86_64-pc-windows-msvc.lib`,
`libkernelx86_64-unknown-linux-gnu.a`, `libkernelaarch64-apple-darwin.a`, ...) —
no compiler is invoked, but the C++ ASTC object is linked in, which is why the
current manifest comment gates it. Its `build-dependencies` (`cc`,
`ispc_compile`, `ispc_rt`) are compiled whenever the crate is built.

Cargo semantics that decide the impact:

- `required-features`: "specifies which features the target needs in order to
  be built. If any of the required features are not enabled, the target will be
  skipped" — `[[bin]]` only. This is what keeps `fetch_maps` out of ordinary
  builds today, and it works identically in a workspace.
- Feature flags with resolver v2/v3: "the features flags allow enabling
  features for any of the packages selected on the command-line with `-p` and
  `--workspace` flags"; "Features of workspace members may be enabled with
  `package-name/feature-name` syntax" (`cargo build` man page). Feature
  unification: "When a dependency is used by multiple packages, Cargo will use
  the union of all features enabled on that dependency when building it".

Options:

1. **Optional feature on the crate that hosts `fetch_maps`** (status quo,
   moved or not). `cargo build --workspace` / `cargo test --workspace` never
   enable non-default features, so `intel_tex_2` is never built unless someone
   passes `--features encode` (root package) or `--features tools/encode` /
   `-p tools --features encode`. Cheapest; recommended.
2. **Separate `crates/tools` crate with `intel_tex_2` as a plain (non-optional)
   dependency.** Then `--workspace` builds/tests compile and link the kernel;
   in a *virtual* workspace without `default-members`, so does plain
   `cargo build` at the root ("all members will be used"). Mitigation is
   `default-members` excluding `tools` (root `cargo build` then skips it, but
   `--workspace` still includes it) — or keep the feature gate (option 1) even
   inside the tools crate. Not recommended unless the tools crate also becomes
   the home for other heavy dev-only deps (`resvg`, `indicatif`, `reqwest`,
   `tokio` "full", `clap`) that the app does not need — that dependency
   trimming is the real argument for a tools crate, and it composes with
   option 1 (`tools` crate + `encode` feature + `required-features`).

Either way `bc7z` (pack/unpack, `zstd`) belongs in core (both the app decoder
and `fetch_maps`' encoder use it: `tarkov_map::bc7z::{pack, unpack, Bc7Image}`).

## (c) rust-embed `folder` resolution across crates

From `rust-embed-impl-8.12.0/src/lib.rs` (`impl_rust_embed`):

```rust
// Base relative paths on the Cargo.toml location
let (relative_path, absolute_folder_path) = if Path::new(&folder_path).is_relative() {
  let absolute_path = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join(&folder_path)...;
  (Some(folder_path.clone()), absolute_path)
} else {
  if cfg!(feature = "compression") {
    match Path::new(&folder_path).strip_prefix(&cargo_manifest_dir) { Ok(rel) => ..., Err(_) =>
      return Err(syn::Error::new_spanned(ast, "`folder` must be a relative path under `compression` feature.")) }
  } else { (None, folder_path) }
};
if !Path::new(&absolute_folder_path).exists() && !allow_missing { /* compile error: folder '...' does not exist. cwd: '...' */ }
```

- `CARGO_MANIFEST_DIR` here is the *crate being compiled* (the one containing
  the derive), so `#[folder = "assets/"]` in `crates/app/src/assets.rs` means
  `crates/app/assets/`. To keep `assets/` at the repo root use
  `#[folder = "../../assets/"]` — relative paths with `..` are joined
  verbatim and pass the `exists()` check. Absolute paths are rejected under
  `compression` unless they are under the manifest dir, so `$CARGO_MANIFEST_DIR`
  interpolation (`interpolate-folder-path` feature) buys nothing here.
- Debug mode: the derive's `dynamic()` impl bakes the compile-time
  `absolute_folder_path` into the generated `get()`
  (`::std::path::Path::new(#folder_path).join(&rel_file_path)`) and reads files
  at runtime from there. It is therefore **not** cwd-relative — running
  `cargo run` from anywhere still finds `assets/`. (The docs.rs README says
  debug builds resolve "relative to where the binary is run from"; the code in
  8.12.0 does not.) `include`/`exclude` (`#[exclude = "maps/*.png"]`) are
  applied by the same `PathMatcher` in both modes.
- Release / `debug-embed`: files are embedded with `include_bytes!` of the
  canonical path, or, with `compression` (enabled in this repo), with
  `include_flate::flate!(... from "<folder>/<rel>")` where the path is relative
  and include-flate resolves it against `CARGO_MANIFEST_DIR` ("Absolute paths
  are not supported", include-flate-codegen 0.3.4). Hence the "must be
  relative" rule above.
- The `assets.rs` tests (`bundled_maps_parse_and_reference_bundled_images`,
  `every_bundled_image_is_a_map_main_floor`) read through the same derive, so
  they keep passing wherever the derive lives as long as `folder` is right.
- `fetch_maps` writes with `repo_path()` = `env!("CARGO_MANIFEST_DIR")/<path>`
  (`assets/maps.ron`, `assets/maps/*.png`, `assets/maps/*.bc7z`). If it moves
  to `crates/tools`, that becomes `crates/tools/assets/`; either point it at
  `../../assets` or add an `--assets-dir` argument.

## (d) build.rs / winres belong to the app crate

Cargo book, "Build Scripts": "Placing a file named `build.rs` in the root of a
package will cause Cargo to compile that script and execute it just before
building the package"; "the build script's current directory is the root
directory of the build script's package"; build dependencies are declared in
the package's own `[build-dependencies]` and "are not available to the package
itself unless also explicitly added in the `[dependencies]` table". So
`build.rs` + `[target.'cfg(windows)'.build-dependencies] winres = "0.1"` move
with the app package; `res.set_icon("assets/tarkov-map-icon.ico")` is resolved
from the app package root (unchanged for the root-package layout;
`../../assets/tarkov-map-icon.ico` if the app lives in `crates/app` and assets
stay at the root). Core/adapters need no build script. Default rerun rule:
"always re-running the build script if any file within the package is changed"
— emit `cargo::rerun-if-changed=assets/tarkov-map-icon.ico` (and `build.rs`)
in the app's `build.rs` so asset regeneration by `fetch_maps` doesn't re-run
the resource compiler needlessly (minor).

## (e) release-please with a Rust workspace

Current config: `.github/release-please-config.json` has one package `"."`,
`release-type: rust`, `bump-minor-pre-major`, `bump-patch-for-minor-pre-major`,
`include-component-in-tag: false`; manifest `{".": "0.1.14"}`; workflow uses
`googleapis/release-please-action@v4` with a PAT. Tags are `vX.Y.Z`,
`CHANGELOG.md` at the root.

What the Rust strategy does (`src/strategies/rust.ts`, `buildUpdates`):

- Reads `Cargo.toml` at the package path. If it has `workspace.members`, it
  logs "found workspace with N members, upgrading all", puts the root
  `package.name` and every member's `package.name` into a `versionsMap` (all
  set to the **same new version**), pushes a `CargoToml` update for each
  member manifest **and for the root `Cargo.toml`**, then a `CargoLock` update
  for `Cargo.lock`. Members are read as `${member}/Cargo.toml` **without glob
  expansion** — a `crates/*` entry yields "member crates/* declared but did
  not find Cargo.toml" and is skipped (harmless: that member is simply not
  bumped).
- Otherwise ("single crate found") it updates `Cargo.toml`, `Cargo.lock`, and
  `CHANGELOG.md` at the package path (`addPath`).
- `CargoToml.updateContent` (`src/updaters/rust/cargo-toml.ts`):
  `if (!parsed.package) { throw new Error('is not a package manifest (might be
  a cargo workspace)') }`, then `replaceTomlValue(payload, ['package',
  'version'], ...)`, then bumps `dependencies`/`dev-dependencies`/
  `build-dependencies` (and `[target.*]` variants) entries whose name is in
  `versionsMap` **and** which have both `path` and `version` (path-only deps
  are skipped with a log line). It never touches `[workspace.package]` or
  `[workspace.dependencies]`.

Consequences:

1. **`"."` + `release-type: rust` + a virtual root manifest fails.** The
   workspace branch pushes a `CargoToml` update for the virtual root, and the
   updater throws. This is release-please issue #1998; the "fix" (PR #2001)
   only added the README note: Rust "workspaces require a manifest driven
   release and the `cargo-workspace` plugin".
2. **`"."` + `release-type: rust` + root package layout works unchanged**: the
   root manifest has `[package]`; literal members get lock-step version bumps
   (fine for unpublished path crates; or list them via a glob to leave their
   versions alone); `Cargo.lock` gets the new `tarkov-map` version; the root
   `CHANGELOG.md` and `v*` tags are unchanged. `"."` receives **all** commits
   (`commit-split.ts`: "The special '.' path... is assigned all commits in
   manifest.ts"), so a `feat:` touching only `crates/core` still bumps and is
   listed. This is the decisive reason to prefer the root-package layout.
3. **Virtual root + app at `crates/app` as the only configured package**
   (`"crates/app": { "release-type": "rust", "include-component-in-tag": false,
   "changelog-path": "/CHANGELOG.md" }` — `addPath` treats a leading `/` as
   repo-root-relative — plus manifest `{"crates/app": "0.1.14"}`): commits are
   attributed **by path** ("Commits that only touch files under paths not
   specified here are ignored"), so changes confined to `crates/core` or
   `crates/adapters` neither bump nor appear in the changelog. Also the
   strategy would update `crates/app/Cargo.lock` (nonexistent →
   `createIfMissing: false`, skipped) and **not** the root `Cargo.lock`, so
   `cargo build --locked` fails after a release PR ("Cargo attempted to change
   the lock file") unless the `cargo-workspace` plugin is enabled — its
   `run()` pushes a root-level `Cargo.lock` update onto the root/first Rust
   candidate.
4. **Virtual root + every crate configured + plugins**: `"plugins":
   [{"type":"cargo-workspace","merge":false},{"type":"linked-versions",
   "groupName":"tarkov-map","components":["tarkov-map","tarkov-map-core",...]}]`,
   `skip-github-release: true` + `skip-changelog: true` on the library
   crates, `include-component-in-tag: false` on the app only (two packages
   with that flag would collide on `vX.Y.Z`). `cargo-workspace` bumps
   dependents (patch) and rewrites root `Cargo.lock`; `linked-versions` forces
   the whole group to the highest version, and for a component with no commits
   of its own it appends a fake commit `chore(<component>): Synchronize
   tarkov-map versions` (`linked-versions.ts`) — so the app's changelog for a
   core-only change reads "Synchronize versions", not the feature. The
   `cargo-workspace` plugin also requires a **literal string** `package.version`
   in every member (`throw new ConfigurationError('... has an invalid
   [package.version]')` when it is a table such as `{ workspace = true }`), and
   the version-in-`.release-please-manifest.json` keys must be rewritten to the
   crate paths.
5. **`"."` + `release-type: simple` + `extra-files: [{type:"toml",
   path:"crates/app/Cargo.toml", jsonpath:"$.package.version"}]`** would keep
   whole-repo commit attribution with a virtual root, but nothing updates
   `Cargo.lock` (drift; `--locked` breaks) and it adds a `version.txt`. Not
   recommended.

Version inheritance caveat (any layout): keep `version = "x.y.z"` literal in
crates release-please edits; `version.workspace = true` is replaced by
`replaceTomlValue` with a literal (silently un-inheriting) and the workspace
table's version is never bumped.

`release.yml` (binary build) under the recommended layout: unchanged in
substance. `cargo build --release` at the root builds the root package
(default-members unset), output stays `target/release/tarkov-map.exe`
(shared `target/`). Making it explicit — `cargo build --release -p tarkov-map`
(or `--bin tarkov-map`) — protects against a later `default-members` change or
a switch to a virtual root, and adding `--locked` makes a stale lock a loud
failure rather than a silent rewrite in CI. If the app ever moves to
`crates/app`, only the `-p`/`--bin` selection matters; the artifact path does
not change.

`ci.yml`: today it only runs `cargo build --release` on Windows. In a
workspace that builds only the root package's dependency closure (core,
adapters) but does not compile core/adapters tests. Add `cargo test
--workspace` (and, if wanted, `cargo build --workspace --features encode`
guarded to a Linux/Windows job to exercise the `intel_tex_2` link) — this is
where the tools/feature decision in (b) shows up.

## (f) Nix packaging

`flake.nix` uses numtide/blueprint with `prefix = "nix/"`; the only file is
`nix/devshell.nix` (X11/Wayland/Vulkan libs, `LD_LIBRARY_PATH`). There is no
package derivation, so nothing breaks. When one is added
(`nix/packages/tarkov-map.nix`), per the nixpkgs manual:

- Use `cargoLock.lockFile = ../../Cargo.lock` (root lock; "buildRustPackage
  also supports vendoring dependencies directly from a `Cargo.lock` file using
  the `cargoLock` argument"; `outputHashes` only for git deps — none today).
- Root-package layout: `cargoBuildFlags = [ "-p" "tarkov-map" ]` (optional
  since the root package is the default). Non-root app: "the relative path to
  the crate to build can be set through the optional `buildAndTestSubdir`
  environment variable" (`buildAndTestSubdir = "crates/app"`).
- `checkFeatures`/`buildFeatures` are separate; leave `encode` off. If a
  `tools` crate hard-depends on `intel_tex_2`, `cargo test --workspace`-style
  checks in the sandbox will link the prebuilt libs (present for
  `x86_64-unknown-linux-gnu`), so it builds but adds nothing useful —
  another vote for keeping the feature gate.
- rust-embed's compile-time folder check needs `assets/` inside `src` of the
  derivation (it is, when `src = ./.` at the repo root).

## Recommended layout and concrete edits

```
Cargo.toml              # [workspace] + [package] tarkov-map (egui app; root package)
Cargo.lock              # single, shared
build.rs                # winres, unchanged
assets/                 # unchanged; rust-embed #[folder = "assets/"] unchanged
src/main.rs, src/...    # egui app (today's src/bin/tarkov-map/*)
src/bin/fetch_maps.rs   # stays behind `encode` + required-features (or -> crates/tools)
crates/core/            # domain + app model + ports (bc7z lives here); no build.rs
crates/adapters/        # port impls
crates/tools/           # OPTIONAL: fetch_maps + heavy dev-only deps, encode feature kept
CHANGELOG.md            # unchanged, root
.github/release-please-config.json   # unchanged ("." / rust)
.github/release-please-manifest.json # unchanged
```

Concrete edits:

- `Cargo.toml`: add `[workspace] members = [...]`, `[workspace.package]`
  (`edition`, `license`), `[workspace.dependencies]`; members use
  `edition.workspace = true`, `dep.workspace = true`, and **literal**
  `version`s. Keep `[features] encode`, `[[bin]] fetch_maps required-features`
  and `[[bin]] tarkov-map` in whichever crate hosts them; drop the explicit
  `path = "src/bin/tarkov-map/main.rs"` if the app moves to `src/main.rs`.
- `src/bin/tarkov-map/assets.rs` (→ app crate): no change to `#[folder]` if
  the app is the root package; `../../assets/` otherwise. `fetch_maps`'
  `repo_path` likewise.
- `.github/workflows/ci.yml`: `cargo build --release -p tarkov-map` +
  `cargo test --workspace`.
- `.github/workflows/release.yml`: `cargo build --release --locked -p
  tarkov-map`; asset copy/upload lines unchanged.
- `.github/workflows/release-please.yml`, `release-please-config.json`,
  `.release-please-manifest.json`: unchanged. (Only if the virtual layout is
  chosen: rewrite per (e).4 and expect the first PR to re-baseline; verify the
  `v0.1.14` tag lookup with `include-component-in-tag: false` on the app.)
- `nix/`: nothing now; `cargoLock.lockFile` + `-p tarkov-map` when a package
  is added.

## Open questions

- Whether to create `crates/tools` at all (dependency-trimming benefit for
  the app: `resvg`, `indicatif`, `reqwest`, `tokio` "full", `clap` are only
  needed by `fetch_maps`) — orthogonal to the release mechanics above as long
  as the `encode` feature gate stays.
- Whether the map issue wants core/adapters versions bumped in lock-step
  (list them literally in `members`) or frozen (use a glob, which the Rust
  strategy skips); neither is observable to users since they are unpublished
  path crates.
