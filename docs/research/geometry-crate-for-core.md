# Geometry crate for typed coordinate spaces in core

Research for wayfinder ticket #37 (parent map issue #35). Question: the core
crate will own the game -> image -> screen maths without any egui types. Compare
`euclid` (typed units), `glam` (blessed.rs pick, `DVec2`) and hand-rolled
newtypes on compile-time separation of coordinate spaces, f64 support, serde
support (`maps.ron`), egui interop cost at the UI edge, maintenance/activity,
dependency weight and ergonomics; recommend one and sketch the types core would
expose.

## Sources

All findings are from primary sources, read on 2026-08-17:

- `euclid` docs — <https://docs.rs/euclid/latest/euclid/>,
  <https://docs.rs/euclid/latest/euclid/struct.Point2D.html>,
  <https://docs.rs/euclid/latest/euclid/struct.Transform2D.html>
- `euclid` repository (`servo/euclid`, `main`): `Cargo.toml`, `src/point.rs`,
  `src/box2d.rs`, `src/rect.rs`, `src/transform2d.rs` —
  <https://github.com/servo/euclid>
- `glam` docs — <https://docs.rs/glam/latest/glam/>; README —
  <https://github.com/bitshifter/glam-rs/blob/main/README.md>
- blessed.rs crate list (Math section) — <https://blessed.rs/crates>
- crates.io API for versions, dates, dependencies, reverse dependencies and
  crate size — <https://crates.io/api/v1/crates/euclid>,
  <https://crates.io/api/v1/crates/glam>
- GitHub API for repo activity — <https://api.github.com/repos/servo/euclid>,
  <https://api.github.com/repos/bitshifter/glam-rs>
- `emath` (egui's maths crate): `Cargo.toml`, `src/pos2.rs`, `src/rect.rs` —
  <https://github.com/emilk/egui/tree/main/crates/emath>
- `ron` deserializer (`src/de/mod.rs`, `deserialize_tuple`) —
  <https://github.com/ron-rs/ron/blob/master/src/de/mod.rs>
- This repo, at `origin/main` (`8fcba7a`): `src/lib.rs`,
  `src/bin/tarkov-map/coordinates.rs`, `src/bin/tarkov-map/ui.rs`,
  `src/bin/tarkov-map/overlays.rs`, `Cargo.toml`.
- A throwaway compile check of the type sketch below (euclid 0.22.14 + ron
  0.12 + serde 1), run locally on 2026-08-17; results quoted in "Sketch".

## TL;DR

- **Recommend `euclid`** with the `serde` feature. It is the only one of the
  three that gives compile-time separation of coordinate spaces *and* a real
  transform type that is typed by source and destination space
  (`Transform2D<f64, Game, Image>`), which is precisely the shape of the
  game -> image -> screen problem. Everything is generic over the scalar, so
  `f64` is native. Serde is a one-feature flag. Its only mandatory dependency is
  `num-traits`.
- `glam` is faster and more actively released, but it has "no generics and
  minimal traits in the public API" by design: `DVec2` is `DVec2` in every
  space, so the compile-time separation the ticket asks for would have to be
  hand-rolled on top of it anyway. It also drags in SIMD-flavoured API surface
  and a ~860 KB crate we would use ~2% of.
- Hand-rolled newtypes are viable for the current ~90 lines of maths but
  reproduce exactly what euclid already provides (typed points, boxes, affine
  transform, inverse, composition, serde), with no external review. Not worth
  it while euclid is maintained.
- egui interop cost is the same for all three: egui's `emath` types are `f32`
  and have `From<[f32; 2]>`/`From<(f32, f32)>` and no euclid/glam impls, so the
  UI edge is `pos.to_f32().to_array().into()` (euclid) or
  `pos.as_vec2().to_array().into()` (glam). Core never names an egui type.

## What core actually needs (from the current code)

The maths today lives in `src/bin/tarkov-map/coordinates.rs` and is used from
`ui.rs` and `overlays.rs`:

- `Map` (in `src/lib.rs`) carries `bounds: Option<[[f64; 2]; 2]>` (game
  coordinates, `[[maxX, minY], [minX, maxY]]`), `coordinate_rotation:
  Option<f64>` (degrees; 90/180/270), `transform: Option<[f64; 4]>` (`[scaleX,
  translateX, scaleY, translateY]`, only used for 270 deg maps), `image_size:
  [f32; 2]` (pixels) and `logical_size: [f32; 2]`; labels/spawns/extracts carry
  `[f64; 2]`/`[f64; 3]` positions. All of this round-trips through `maps.ron`
  via `ron` + `serde`.
- `game_to_display(map, map_rect: egui::Rect, game_pos: [f64; 2]) ->
  Option<egui::Pos2>` does: rotate the game point by `coordinate_rotation`;
  either (a) apply the tarkov-dev `transform` and divide by `image_size` or (b)
  rotate the four bounds corners, take the rotated extent and normalise (Y
  inverted); then lerp the resulting fraction into `map_rect`.
- `ui.rs::show_map` builds `map_rect` (screen space, `f32`) from
  `viewport_rect`, a fit scale, `zoom` and `pan_offset`, and calls
  `game_to_display` for centre-on-player; `overlays.rs` calls it once per
  label/spawn/extract/player and does a little screen-space `f32` vector maths
  for the marker triangle.

So core needs, without egui: a game-space point and box, an image-space point,
size and rect, a screen-space point and rect (for the UI to hand in the
`map_rect` and get positions back), and two affine transforms
(game -> image, image -> screen) that compose. It does not need 3D, quaternions,
SIMD or matrices beyond 3x2 affine.

## Comparison

| Criterion | euclid 0.22.14 | glam 0.33.3 | hand-rolled |
| --- | --- | --- | --- |
| Compile-time space separation | Yes, built in: "All types are generic over the scalar type of their component (`f32`, `i32`, etc.), and tagged with a generic Unit parameter which is useful to prevent mixing values from different spaces" (docs.rs/euclid). `Transform2D<T, Src, Dst>::transform_point(Point2D<T, Src>) -> Point2D<T, Dst>`; `then::<NewDst>` chains transforms and the compiler rejects `Game -> Image` applied to an `ImagePos`. | No. README: "No generics and minimal traits in the public API for simplicity of usage"; `DVec2` is one concrete type for every space. Separation would need our own `struct GamePos(DVec2)` newtypes plus hand-written ops, or a marker generic wrapper — i.e. option 3 built on glam. | Yes, by construction (`struct GamePos { x: f64, y: f64 }` etc.), but every operator, `lerp`, `Rect` helper and the transform type must be written and tested by us. |
| f64 | Native: every type is `Foo<T, U>`; `Point2D<f64, U>`, `Transform2D<f64, Src, Dst>`; `.to_f32()` / `.cast::<f32>()` for the UI edge (docs.rs Point2D). | Native: `DVec2`, `DMat3`, `DAffine2` (docs.rs/glam); `.as_vec2()` to f32. | Trivially. |
| serde (`maps.ron`) | Optional `serde` feature (Cargo.toml: `serde = { version = "1.0", default-features = false, features = ["serde_derive"], optional = true }`). `Point2D`/`Vector2D` serialise as a 2-tuple, `Box2D` as `(min: (..), max: (..))`, `Transform2D`/`Rect` as derived structs; bounds are `T: Serialize`, the unit type needs nothing (`src/point.rs`, `src/box2d.rs`, `src/transform2d.rs`). RON prints a tuple as `(x, y)` and only accepts `(` for tuples (`ron/src/de/mod.rs` `deserialize_tuple`), so `maps.ron` would change from `[x, y]` to `(x, y)` — a regeneration by `fetch_maps`, not a compatibility problem since the file is generated and embedded. | Optional `serde` feature: "implementations of Serialize and Deserialize for all glam types"; `DVec2` also serialises as a tuple. Same RON consequence. | Derive `Serialize`/`Deserialize` on our own structs; free choice of representation (could keep `[x, y]` via `serde(from/into)` or tuple-struct). |
| egui interop at the UI edge | egui `Pos2`/`Vec2`/`Rect` are `f32` (`emath/src/rect.rs`: `Rect { min: Pos2, max: Pos2 }`) with `From<[f32; 2]>`, `From<(f32, f32)>` and, only behind emath's optional `mint` feature, `From<mint::Point2<f32>>` (`emath/src/pos2.rs`, `emath/Cargo.toml`). Cost with euclid: `p.to_f32().to_array().into()` / `egui::pos2(p.x as f32, p.y as f32)`; the reverse `ScreenPos::new(r.min.x as f64, r.min.y as f64)`. Two tiny `From` impls in the UI crate hide it. euclid also has a `mint` feature if we ever turn on egui's. | Same shape: `v.as_vec2().to_array().into()`; glam also has `mint`. | Same: write `From<GamePos>`-style helpers in the UI crate. |
| Maintenance / activity | Servo project (`servo/euclid`), 491 stars, 15 open issues, last push 2026-03-17; releases 0.22.13 (2026-01-19) and 0.22.14 (2026-03-18); the 0.22 line has been semver-compatible since 0.22.0 (2020-07-29). 232 reverse dependencies, 56M downloads. MSRV 1.63. Low churn, stable API; used by Servo/WebRender. | `bitshifter/glam-rs`, 2032 stars, 15 open issues, last push 2026-08-17; releases 0.33.3 (2026-08-03), 0.33.2 (2026-06-28), 0.33.1 (2026-06-06); breaking minor bumps roughly 2-3 per year (0.30.0 2025-02-18, 0.31.0 2026-01-21, 0.32.0 2026-02-11, 0.33.0 2026-05-21). 1197 reverse dependencies, 123M downloads. MSRV 1.68.2. blessed.rs lists it under Math: "Fast math library optimised for game development use cases". Very active, but pre-1.0 with regular breaking releases. | Ours to maintain; no upstream churn, no upstream fixes. |
| Dependency weight | Required: `num-traits` only (`default-features = false`); optional `serde`, `mint`, `bytemuck`, `arbitrary`, `malloc_size_of` (crates.io deps for 0.22.14). Crate size 88 KB. `serde` is already in the tree; `num-traits` is almost certainly already there transitively (image/resvg). | Required: none ("Minimal dependencies — all external crates are optional"); optional `serde_core`, `mint`, `bytemuck`, `libm`, `rand`, `rkyv`, `approx`, `encase`, `zerocopy`, `speedy`, `arbitrary`. Crate size 858 KB (SIMD code paths for f32 types). | Zero. |
| Ergonomics for this problem | `Box2D::outer_transformed_box`, `Rect`, `Size2D`, `Transform2D::{rotation, scale, translation, then, then_translate, then_scale, pre_scale, inverse, with_destination}`, `Angle::degrees`, `Point2D::lerp`. Reads as the maths in `coordinates.rs`. Type-parameter noise is tamed with `type` aliases. `Transform2D` is a column-major 3x3 compressed to 3x2 with translation in `m31, m32` (docs.rs Transform2D) — same as tarkov-dev's `[scaleX, translateX, scaleY, translateY]` idea. | `DAffine2` covers the transform (`from_scale_angle_translation`, `inverse`, `*` composition), `DVec2` the points; no `Rect`/`Box2D` (would need our own). Very good numeric ergonomics, none for spaces. | Whatever we write; realistically a `Transform` struct with 6 `f64`s and `apply`/`inverse`/`then`. |

## Recommendation

Use **`euclid = { version = "0.22", features = ["serde"] }`** in core.

Why, in the order the ticket ranks the criteria:

1. Compile-time separation is the point of the ticket, and euclid is the only
   candidate that ships it: units on points, sizes, boxes and, decisively, on
   transforms (`Src`/`Dst`). With glam we would write the newtypes ourselves and
   then lose glam's operators on them; with hand-rolled types we would write
   euclid.
2. `f64` end to end, `.to_f32()` at the UI edge, no `as` scattering inside core.
3. serde is a feature flag; the only visible effect is `[x, y]` becoming
   `(x, y)` in the generated `maps.ron`.
4. Interop with egui costs the same two conversion helpers whichever option is
   chosen; core stays egui-free.
5. Maintenance: quieter than glam but alive (two releases in 2026, Servo-backed)
   and API-stable for six years, which for a 2D-affine-only consumer is a
   feature, not a risk. If it ever stalls, the surface we use is small enough
   to vendor as hand-rolled types behind the same aliases.
6. Dependency weight is one small crate (`num-traits`) we almost certainly
   already compile.

Not recommended: `glam` — right crate for a game engine's f32 SIMD maths, wrong
shape for typed 2D map spaces; `nalgebra` (also on blessed.rs) for the same
reason plus far more weight. Hand-rolled — keep as the fallback plan only.

## Sketch: what core would expose

Marker units are zero-sized structs. Everything below compiled and ran against
euclid 0.22.14 (`(bounds:(min:(-100.0,-50.0),max:(100.0,50.0)),rotation_deg:180.0,image_size:(2000.0,1000.0))`
round-tripped through `ron`; game `(100, 50)` on a 180 deg map mapped to image
`(0, 1000)`, matching `coordinates.rs`; `Transform2D::rotation(Angle::degrees(90.0))`
sends `(1, 0)` to `(0, 1)`, the same direction as `rotate_point`; the 270 deg
`svg_transform` branch with `[2, 100, 3, 50]` sends game `(1, 0)` to image
`(100, 53)`, matching the `scale_y = -transform[2]` convention in
`coordinates.rs`).

```rust
use euclid::{Angle, Box2D, Point2D, Rect, Size2D, Transform2D, Vector2D};

/// Tarkov world space, metres. `x` is game X, `y` is game Z (height is dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Game;
/// Pixel space of the map PNG: origin top-left, y down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Image;
/// Screen/UI points. Core never constructs these itself; the UI hands them in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Screen;

pub type GamePos     = Point2D<f64, Game>;
pub type GameVec     = Vector2D<f64, Game>;
pub type GameBounds  = Box2D<f64, Game>;      // replaces bounds: [[f64; 2]; 2]
pub type ImagePos    = Point2D<f64, Image>;
pub type ImageSize   = Size2D<f64, Image>;    // replaces image_size: [f32; 2]
pub type ImageRect   = Rect<f64, Image>;
pub type ScreenPos   = Point2D<f64, Screen>;
pub type ScreenRect  = Rect<f64, Screen>;     // the UI's map_rect, converted once
pub type GameToImage = Transform2D<f64, Game, Image>;
pub type ImageToScreen = Transform2D<f64, Image, Screen>;
pub type GameToScreen  = Transform2D<f64, Game, Screen>;

/// Everything `Map` needs to place a game point on its image.
/// Serialises into maps.ron as before (positions become `(x, y)` tuples).
#[derive(Serialize, Deserialize)]
pub struct Projection {
    pub bounds: GameBounds,
    pub rotation: f64,                       // degrees, 90/180/270
    pub svg_transform: Option<[f64; 4]>,     // tarkov-dev [sx, tx, sy, ty] (270 deg maps)
    pub image_size: ImageSize,
}

impl Projection {
    /// game -> image pixels; mirrors today's `game_to_display` minus the screen lerp.
    pub fn game_to_image(&self) -> GameToImage {
        let rot: Transform2D<f64, Game, Game> =
            Transform2D::rotation(Angle::degrees(self.rotation));
        if self.rotation == 270.0
            && let Some([sx, tx, sy, ty]) = self.svg_transform
        {
            return rot.then_scale(sx, -sy).then_translate(Vector2D::new(tx, ty))
                      .with_destination::<Image>();
        }
        let rb = rot.outer_transformed_box(&self.bounds);
        rot.then_translate(Vector2D::new(-rb.min.x, -rb.max.y))     // y inverted
           .then_scale(self.image_size.width / rb.width(),
                       -self.image_size.height / rb.height())
           .with_destination::<Image>()
    }
}

/// image pixels -> screen; `map_rect` is the on-screen image rect (fit * zoom + pan).
pub fn image_to_screen(image_size: ImageSize, map_rect: ScreenRect) -> ImageToScreen {
    Transform2D::scale(map_rect.width() / image_size.width,
                       map_rect.height() / image_size.height)
        .then_translate(map_rect.origin.to_vector())
}

// Usage in core (no egui):
// let g2s: GameToScreen = proj.game_to_image().then(&image_to_screen(size, map_rect));
// let s: ScreenPos = g2s.transform_point(GamePos::new(x, z));
// g2s.inverse()  -> Option<Transform2D<f64, Screen, Game>>  (hit-testing / hover coords)
// g2s.transform_point(ImagePos::new(..))  -> compile error: expected `Game`, found `Image`
```

At the UI edge (in the egui binary only):

```rust
fn to_egui(p: ScreenPos) -> egui::Pos2 { p.to_f32().to_array().into() }
fn from_egui(r: egui::Rect) -> ScreenRect {
    ScreenRect::new(ScreenPos::new(r.min.x as f64, r.min.y as f64),
                    Size2D::new(r.width() as f64, r.height() as f64))
}
```

Notes for the implementer:

- Keep `Game` as 2D (x, z); `Spawn`/`Extract` 3D positions can stay `[f64; 3]`
  or become `Point3D<f64, Game>` and be projected with `.xz()`-style helpers;
  CONTEXT.md says height is displayed but never used to choose what is drawn.
- `Rect` vs `Box2D`: euclid's `Rect` is origin + size, `Box2D` is min + max.
  Use `Box2D` for game bounds (given as corners) and `Rect` for image/screen
  rects (given as origin + size), which is what egui's `Rect::from_min_size`
  produces.
- The `logical_size: [f32; 2]` used only for zoom fitting can become
  `Size2D<f32, Image>` or stay an array; it is not part of the projection.
- If the UI later enables `emath`'s `mint` feature, euclid's `mint` feature
  gives `From`/`Into` both ways for free and the two helpers above disappear.
