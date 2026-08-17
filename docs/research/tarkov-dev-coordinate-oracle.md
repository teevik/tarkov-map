# tarkov.dev interactive map: coordinate maths as a test oracle

Research for wayfinder ticket #39 (parent map issue #35). Question: locate the
upstream implementation that `src/bin/tarkov-map/coordinates.rs`
(`game_to_display`) claims to follow, document the exact algorithm (rotation,
bounds, transform, y-inversion), compare it with the Rust port, and decide
whether golden test vectors (game position -> image fraction/pixel per map,
incl. Labs / Labyrinth / Factory) can be derived to serve as an oracle for the
core's coordinate tests.

## Sources

All findings are from primary sources, read on 2026-08-17:

- `the-hideout/tarkov-dev` at commit `d3dc9b8401c9a4312dc5cd6b4e52e0a4e398a5cb`
  (2026-08-13). Files:
  - `src/pages/map/index.jsx` — `getCRS`, `applyRotation`, `pos`, `getBounds`
    (L44-L135), map/overlay setup (L838-L925), player-position marker (L2160-L2200)
    — <https://github.com/the-hideout/tarkov-dev/blob/d3dc9b8401c9a4312dc5cd6b4e52e0a4e398a5cb/src/pages/map/index.jsx>
    (last touched by `9fd29f7f16e5d5408831d9be48274dcb8862de1d`, 2026-07-20).
  - `src/data/maps.json` — the per-map `bounds`, `svgBounds`, `transform`,
    `coordinateRotation`, `tilePath`, `tileSize`, `svgPath` (this is where
    tarkov-map's `assets/maps.ron` fields come from; the API does not carry them)
    — <https://github.com/the-hideout/tarkov-dev/blob/d3dc9b8401c9a4312dc5cd6b4e52e0a4e398a5cb/src/data/maps.json>
  - `src/features/maps/index.js` L30/L238 (`rawMapData` = `maps.json`, merged onto
    the API map by `normalizedName`), `package.json` (`leaflet ^1.9.4`).
- Leaflet 1.9.4 `dist/leaflet-src.js` (the code that actually turns lat/lng into
  pixels): `CRS.latLngToPoint` (L1600), `Transformation._transform` (L1831),
  `CRS.Simple` (L6759), `ImageOverlay._reset` (L9565), `SVGOverlay` (L9737),
  `GridLayer._getTilePos` (L11994) — <https://github.com/Leaflet/Leaflet/blob/v1.9.4/src/>.
- `the-hideout/tarkov-dev-svg-maps` at commit
  `5a8b6115d1c0cf56f2ebaac1a96fa5ae3074d178` (2026-02-16): only the SVGs and a
  README; it contains **no** coordinate metadata and no transform code —
  <https://github.com/the-hideout/tarkov-dev-svg-maps>. The SVGs the site serves
  (`https://assets.tarkov.dev/maps/svg/<Map>.svg`) were fetched to read their
  `viewBox` (Customs `0 0 1062.4827 535.17401`, StreetsOfTarkov `0 0 605.32395
  831.57753`, Factory `0 0 130.81831 141.23242`, Reserve `0 0 827.28742
  761.16437`); none sets `preserveAspectRatio`.
- tarkov-dev has **no unit tests** for the CRS code (`src/pages/map/` holds only
  `index.jsx`, `index.css`, `map-images.mjs`); the only debugging aid is the
  `showMarkersBounds` flag (L39, L1783) that logs a suggested `bounds` array.

## TL;DR

- Upstream is *not* a hand-written game->pixel formula; it is Leaflet's
  `CRS.Simple` with (a) a custom `projection` that rotates `(x, z)` by
  `coordinateRotation` and (b) a `Transformation(transform[0], transform[1],
  -transform[2], transform[3])`. Everything (markers, SVG overlay corners, tiles)
  goes through that one pipeline, so **where a marker lands relative to the base
  image is a consequence of how the base image itself is placed**, and there are
  two different placements:
  - **Tile pyramid** (`tilePath`): tiles sit at fixed CRS pixels, so at zoom 0 the
    image fraction is simply `(sx*rx + mx, -sy*ry + my) / tileSize`.
  - **SVG overlay** (`svgPath`): the SVG element is stretched to the pixel
    rectangle of `svgBounds ?? bounds`; the transform cancels out and the fraction
    inside that box is `(rx - rminX)/(rmaxX - rminX)`, `(rmaxY - ry)/(rmaxY - rminY)`,
    with the browser letterboxing the SVG `viewBox` inside the box (`xMidYMid meet`).
- The Rust port matches upstream **exactly** for the tile-only 270° maps (Labs,
  Labyrinth) and matches the SVG *box* fraction exactly for the 90°/180° SVG maps
  (Customs, Factory, Streets, ...). Discrepancies found:
  1. **Reserve**: upstream places the SVG at `svgBounds` `[[289,-274],[-303,272]]`,
     not `bounds` `[[289,-293],[-303,244]]`; `maps.ron` has no `svgBounds`, so the
     port is ~4-5 % of image height (≈ 24 m, 27-38 SVG px) off vertically.
  2. **Icebreaker**: tile-only, 180°, non-uniform transform `[2,125,3.5,91]`, and
     its `bounds` are a copy of Factory's; upstream positions everything by the
     transform, the port uses the bounds path -> off by 5-13 % at the origin and by
     0.5 (!) at a bounds corner. The Rust special case is keyed on `rotation ==
     270`, but the real upstream distinction is "image is a tile pyramid".
  3. **Letterboxing** on SVG maps: sub-percent (Customs y ≤ 0.47 %, Streets x
     ≤ 0.08 %, Factory y ≤ 0.04 %); the port stretches the image to the bounds
     aspect instead. Only matters if the oracle tolerance is tighter than ~0.5 %.
- Golden vectors **are feasible** and were computed by running upstream's own
  `getCRS`/`applyRotation`/`pos`/`getBounds` verbatim on real Leaflet 1.9.4:
  **38 vectors across 7 maps** (Customs, Streets, Factory, Reserve, Labs,
  Labyrinth, Icebreaker), each with the zoom-0 CRS pixel, the upstream image
  fraction / pixel, and the value the current Rust code produces.

## (a) The upstream algorithm, step by step

`index.jsx` L44-L90 (verbatim):

```js
function getCRS(mapData) {
    let scaleX = 1, scaleY = 1, marginX = 0, marginY = 0;
    if (mapData) {
        if (mapData.transform) {
            scaleX = mapData.transform[0];
            scaleY = mapData.transform[2] * -1;
            marginX = mapData.transform[1];
            marginY = mapData.transform[3];
        }
    }
    return L.extend({}, L.CRS.Simple, {
        transformation: new L.Transformation(scaleX, marginX, scaleY, marginY),
        projection: L.extend({}, L.Projection.LonLat, {
            project: (latLng) => L.Projection.LonLat.project(applyRotation(latLng, mapData.coordinateRotation)),
            unproject: (point) => applyRotation(L.Projection.LonLat.unproject(point), mapData.coordinateRotation * -1),
        }),
    });
}
function applyRotation(latLng, rotation) {
    if (!latLng.lng && !latLng.lat) return L.latLng(0, 0);
    if (!rotation) return latLng;
    const a = (rotation * Math.PI) / 180, cos = Math.cos(a), sin = Math.sin(a);
    const { lng: x, lat: y } = latLng;
    return L.latLng(x * sin + y * cos, x * cos - y * sin);   // (lat = rotatedY, lng = rotatedX)
}
function pos(position) { return [position.z, position.x]; } // Leaflet [lat, lng]
function getBounds(bounds) {
    return L.latLngBounds([bounds[0][1], bounds[0][0]], [bounds[1][1], bounds[1][0]]);
}
```

So, for a game position `(x, y_height, z)`:

1. **Lat/lng**: `lat = z`, `lng = x` (`pos`). Height is ignored for placement.
2. **Rotation** (`projection.project`): `(rx, ry) = (x cos θ - z sin θ, x sin θ + z cos θ)`
   with `θ = coordinateRotation` (0/90/180/270 in the data). This is a plain
   counter-clockwise rotation of the `(x, z)` vector — the same `rotate_point`
   as the Rust port. Note the odd guard: a position at exactly `(0, 0)` skips the
   rotation, which is harmless because `R(0,0) = (0,0)`.
3. **Affine transform** (`Transformation._transform`, Leaflet L1831):
   `px = 2^zoom * (transform[0] * rx + transform[1])`,
   `py = 2^zoom * (-transform[2] * ry + transform[3])`.
   `CRS.Simple`'s default is `Transformation(1, 0, -1, 0)` (y flipped); the site
   keeps the y-flip by negating `transform[2]`. This "zoom-0 CRS pixel" is the
   coordinate everything below is expressed in.
4. **Base image placement** — this is where the two cases split:
   - **Tiles** (`L.tileLayer(tilePath, { tileSize, bounds })`, L871): Leaflet
     places tile `(tx, ty)` at zoom `zoom` at CRS pixel `tx*tileSize ..
     (tx+1)*tileSize` (`GridLayer._getTilePos`, L11994), i.e. tile (0,0) at zoom 0
     covers CRS pixels `[0, tileSize)²`. The `bounds` option only *limits which
     tiles are requested*; it does not move them. Hence, for an image that is the
     tile pyramid composed at any zoom (which is what `fetch_maps.rs`
     `process_tile_map` builds, reporting `imageSize = tileSize`):
     `fx = px / tileSize`, `fy = py / tileSize`.
   - **SVG** (`L.svgOverlay(svgElement, mapData.svgBounds ? getBounds(svgBounds) : bounds)`,
     L891/L917): `ImageOverlay._reset` (Leaflet L9565) computes
     `Bounds(latLngToLayerPoint(NW), latLngToLayerPoint(SE))`, positions the
     element at `bounds.min` and sets `style.width/height` to the box size. NW =
     (max lat, min lng), SE = (min lat, max lng) of `L.latLngBounds` (which
     normalises min/max, so the order of the two corners in `maps.json` does not
     matter). Because the transform is affine with `transform[0] > 0` and
     `-transform[2] < 0`, the marker's fraction inside that box reduces to
     `fx = (rx - rminX) / (rmaxX - rminX)`, `fy = (rmaxY - ry) / (rmaxY - rminY)`
     where `rmin/rmax` are the extents of the rotated bounds corners — the
     transform's scale and margins cancel. (For θ ∈ {0, 90, 180, 270} the two
     projected corners NW/SE span the same rectangle as all four rotated corners,
     so "rotate four corners, take min/max" is equivalent.)
     The `<svg>` element gets `viewBox` copied from the file (L925) and no
     `preserveAspectRatio`, so the browser applies the default `xMidYMid meet`:
     uniform scale `s = min(boxW/vbW, boxH/vbH)`, centred. Fraction inside the SVG
     image: `fx_img = (fx*boxW - (boxW - s*vbW)/2) / (s*vbW)` and likewise for y.
   - When both `tilePath` and `svgPath` exist the user picks; the default is
     `style: "svg"` (see the layers note, `docs/research/tarkov-dev-map-layers.md`
     on branch `research/tarkov-dev-map-layers`), which matches `fetch_maps.rs`
     preferring the SVG.
5. **Marker** = `L.marker(pos(position))` -> the same `latLngToLayerPoint`. So the
   marker/base-image relation is exactly steps 2-4.

Side note (facing, not position): the player-position arrow is rotated by
`playerPosition.rotation + coordinateRotation`, plus a further `+180` when
`coordinateRotation` is 90 or 270 (L2165-L2169).

### Which maps are which (maps.json @ d3dc9b8)

| Map | rot | transform `[sx, mx, sy, my]` | bounds | tilePath | svgPath | notes |
| --- | --- | --- | --- | --- | --- | --- |
| streets-of-tarkov | 180 | `[0.38, 0, 0.38, 0]` | `[[323,-295],[-280,532]]` | no | yes | SVG only |
| ground-zero | 180 | `[0.524, 167.3, 0.524, 65.1]` | `[[249,-124],[-99,364]]` | yes (256) | yes | |
| customs | 180 | `[0.239, 168.65, 0.239, 136.35]` | `[[698,-307],[-372,237]]` | yes (256) | yes | |
| factory | 90 | `[1.629, 119.9, 1.629, 139.3]` | `[[77,-64.5],[-65.5,67.4]]` | yes (256) | yes | |
| icebreaker | 180 | `[2, 125, 3.5, 91]` | `[[77,-64.5],[-65.5,67.4]]` | yes (256) | **no** | bounds copied from Factory; non-uniform scale |
| interchange | 180 | `[0.265, 150.6, 0.265, 134.6]` | `[[598,-442],[-433,426]]` | yes (256) | yes | |
| the-lab | 270 | `[0.575, 281.2, 0.575, 193.7]` | `[[-80,-477],[-287,-193]]` | yes (**175**) | **no** | |
| the-labyrinth | 270 | `[2.115, 85.5, 2.115, 128]` | `[[-52,-37],[53,76]]` | yes (256) | **no** | |
| lighthouse | 180 | `[0.2, 0, 0.2, 0]` | `[[515,-998],[-545,725]]` | no | yes | |
| reserve | 180 | `[0.395, 122, 0.395, 137.65]` | `[[289,-293],[-303,244]]` | yes (256) | yes | **`svgBounds` `[[289,-274],[-303,272]]`** |
| shoreline | 180 | `[0.16, 83.2, 0.16, 111.1]` | `[[504,-415],[-1056,618]]` | yes (256) | yes | |
| terminal | 180 | `[0.2, 0, 0.2, 0]` | `[[463,-580],[-433,475]]` | no | yes | |
| woods | 180 | `[0.1855, 112.95, 0.1855, 167.85]` | `[[646,-914],[-761,442]]` | yes (256) | yes | |

Reserve is the only map with `svgBounds`. `assets/maps.ron` carries `bounds`,
`transform`, `coordinateRotation` verbatim but **not** `svgBounds`
(`fetch_maps.rs` never reads it).

## (b) Comparison with the Rust port (`coordinates.rs`)

| Aspect | Upstream | Rust port | Verdict |
| --- | --- | --- | --- |
| Rotation | `applyRotation` on `(lng=x, lat=z)` | `rotate_point(x, z, rot)` | identical |
| y-inversion | `Transformation(_, _, -transform[2], _)` (tiles); falls out as `(rmaxY - ry)/height` for the SVG box | `frac_y = (rmax_y - ry)/height`; `scale_y = -transform[2]` in the special case | identical |
| Tile-pyramid images | `px/tileSize` (transform-based), regardless of rotation | transform-based **only if `rotation == 270`** | Labs, Labyrinth: exact. **Icebreaker (180°, tiles): wrong** |
| SVG images | box = `svgBounds ?? bounds`, then `xMidYMid meet` letterbox | box = `bounds`, image stretched to `logicalSize` (= bounds size) | Customs/Streets/Factory: exact box fraction, ≤ 0.5 % letterbox error. **Reserve: wrong box** (4-5 % of height) |
| Zero-position guard | `(0,0)` skips rotation (no effect) | none | n/a |

The Rust code comment "For 270° rotation maps with transform ... handles SVG
padding/margins in maps like Labs and Labyrinth" is a misdiagnosis: neither map
has an SVG; the transform path is right because their image is the **tile
pyramid**. The correct predicate is "the map image is tiles" (Labs, Labyrinth,
Icebreaker) versus "the map image is the SVG" (everything else). Numerically the
special case is necessary — the bounds path would put Labs' Medical Block
Elevator at `(0.4684, 0.8434)` instead of `(0.4766, 0.7375)`.

## (c) Golden vectors

### Method

A throwaway Node script (not committed) ran the four upstream functions
**verbatim** on Leaflet 1.9.4 (`bun add leaflet@1.9.4 jsdom`; jsdom only to
satisfy Leaflet's `window`/`document` on load) and, for each test position:

- `crsPx0 = crs.latLngToPoint(L.latLng(pos({x, z})), 0)` — the zoom-0 CRS pixel;
- **tile maps**: `tileFrac = crsPx0 / tileSize` (image fraction of the composed
  tile pyramid, which is what `fetch_maps.rs` bundles);
- **SVG maps**: `box = L.bounds(crs.latLngToPoint(llb.getNorthWest(), 0),
  crs.latLngToPoint(llb.getSouthEast(), 0))` with `llb = getBounds(svgBounds ??
  bounds)` (exactly `ImageOverlay._reset`), `boxFrac = (crsPx0 - box.min) /
  box.size`, then the `xMidYMid meet` correction using the SVG's real `viewBox`
  size -> `imgFrac`, and `imgPx = imgFrac * viewBox` (SVG user units; the bundled
  PNG is rendered at `SVG_RENDER_SCALE` = 2×, so multiply by 2 for PNG pixels).

Alongside, a line-for-line JS translation of `game_to_display` (fraction only)
was evaluated on the same inputs to produce the "current Rust" column.

Test positions per map: the origin, the two `bounds` corners as written in
`maps.json` (a useful sanity anchor: on SVG maps without `svgBounds` they must
map to box fractions 0/1), and up to three real extract positions taken from
`assets/maps.ron` (i.e. the tarkov.dev API), so a core test can reuse the very
same coordinates the app already ships.

Column key: "upstream image fraction" is the authoritative oracle value (`tileFrac`
for Labs/Labyrinth/Icebreaker, `imgFrac` for the SVG maps); "upstream image px" is
`crsPx0` for tile maps and `imgPx` (SVG user units) for SVG maps; "diff" =
current Rust minus upstream. `Position` values are game `(x, z)`; the fraction is
`(fx, fy)` with `(0,0)` = top-left of the image, y down.

### Vectors (38)

**customs** — 180°, SVG (`viewBox` 1062.4827 × 535.17401), box = `bounds`

| Position | game (x, z) | zoom-0 CRS px (x, y) | upstream image fraction (fx, fy) | upstream image px | current Rust (fx, fy) | diff |
| --- | --- | --- | --- | --- | --- | --- |
| origin | (0, 0) | (168.6500, 136.3500) | (0.6523, 0.5649) | (693.0962, 302.3411) | (0.6523, 0.5643) | (0.0000, -0.0006) |
| bounds[0] (x=698, z=-307) | (698, -307) | (1.8280, 62.9770) | (0.0000, -0.0047) | (0.0000, -2.5021) | (0.0000, 0.0000) | (0.0000, 0.0047) |
| bounds[1] (x=-372, z=237) | (-372, 237) | (257.5580, 192.9930) | (1.0000, 1.0047) | (1062.4827, 537.6761) | (1.0000, 1.0000) | (0.0000, -0.0047) |
| extract "ZB-013" (pmc) | (200.9755, -153.086456) | (120.6169, 99.7623) | (0.4645, 0.2809) | (493.5326, 150.3302) | (0.4645, 0.2829) | (-0.0000, 0.0020) |
| extract "Dorms V-Ex" (pmc) | (181.08, 213.25) | (125.3719, 187.3167) | (0.4831, 0.9606) | (513.2884, 514.0929) | (0.4831, 0.9563) | (-0.0000, -0.0043) |
| extract "ZB-1011" (pmc) | (621.4962, -128.604919) | (20.1124, 105.6134) | (0.0715, 0.3263) | (75.9663, 174.6397) | (0.0715, 0.3279) | (-0.0000, 0.0016) |

(Box fractions without the letterbox correction — what the port computes — are
`(0.6523, 0.5643)`, `(0, 0)`, `(1, 1)`, `(0.4645, 0.2829)`, `(0.4831, 0.9563)`,
`(0.0715, 0.3279)`; the SVG is 0.9 % wider than the bounds aspect, so it is
letterboxed 2.5 SVG px top and bottom.)

**streets-of-tarkov** — 180°, SVG only (`viewBox` 605.32395 × 831.57753), transform has zero margins

| Position | game (x, z) | zoom-0 CRS px (x, y) | upstream image fraction (fx, fy) | upstream image px | current Rust (fx, fy) | diff |
| --- | --- | --- | --- | --- | --- | --- |
| origin | (0, 0) | (0.0000, 0.0000) | (0.5357, 0.3567) | (324.2810, 296.6329) | (0.5357, 0.3567) | (-0.0001, -0.0000) |
| bounds[0] (x=323, z=-295) | (323, -295) | (-122.7400, -112.1000) | (-0.0008, 0.0000) | (-0.5069, 0.0000) | (0.0000, 0.0000) | (0.0008, 0.0000) |
| bounds[1] (x=-280, z=532) | (-280, 532) | (106.4000, 202.1600) | (1.0008, 1.0000) | (605.8308, 831.5775) | (1.0000, 1.0000) | (-0.0008, 0.0000) |
| extract "Entrance to Catacombs" (scav) | (-249.008026, 243.797) | (94.6230, 92.6429) | (0.9494, 0.6515) | (574.6673, 541.7793) | (0.9486, 0.6515) | (-0.0008, -0.0000) |
| extract "Ventilation Shaft" (scav) | (-124.127106, 423.93) | (47.1683, 161.0934) | (0.7419, 0.8693) | (449.0951, 722.9094) | (0.7415, 0.8693) | (-0.0004, 0.0000) |
| extract "Sewer Manhole" (scav) | (276.113, 345.389984) | (-104.9229, 131.2482) | (0.0770, 0.7744) | (46.6397, 643.9346) | (0.0778, 0.7744) | (0.0007, 0.0000) |

**factory** — 90°, SVG (`viewBox` 130.81831 × 141.23242), box = `bounds`

| Position | game (x, z) | zoom-0 CRS px (x, y) | upstream image fraction (fx, fy) | upstream image px | current Rust (fx, fy) | diff |
| --- | --- | --- | --- | --- | --- | --- |
| origin | (0, 0) | (119.9000, 139.3000) | (0.5110, 0.5404) | (66.8473, 76.3191) | (0.5110, 0.5404) | (0.0000, -0.0000) |
| bounds[0] (x=77, z=-64.5) | (77, -64.5) | (224.9705, 13.8670) | (1.0000, -0.0004) | (130.8183, -0.0495) | (1.0000, 0.0000) | (0.0000, 0.0004) |
| bounds[1] (x=-65.5, z=67.4) | (-65.5, 67.4) | (10.1054, 245.9995) | (0.0000, 1.0004) | (0.0000, 141.2819) | (0.0000, 1.0000) | (0.0000, -0.0004) |
| extract "Cellars" (pmc) | (73.89422, -29.0818882) | (167.2744, 18.9263) | (0.7315, 0.0215) | (95.6907, 3.0308) | (0.7315, 0.0218) | (-0.0000, 0.0003) |
| extract "Gate 3" (scav) | (58.709, 60.86811) | (20.7458, 43.6630) | (0.0495, 0.1281) | (6.4783, 18.0915) | (0.0495, 0.1284) | (0.0000, 0.0003) |
| extract "Gate 3" (pmc) | (58.43222, 63.29811) | (16.7874, 44.1139) | (0.0311, 0.1300) | (4.0683, 18.3660) | (0.0311, 0.1303) | (0.0000, 0.0003) |

(Rotation 90° swaps the axes: `bounds[0]` = (max x, min z) lands at the *right*
edge, top; `bounds[1]` at the left edge, bottom.)

**reserve** — 180°, SVG (`viewBox` 827.28742 × 761.16437), box = **`svgBounds`** `[[289,-274],[-303,272]]`

| Position | game (x, z) | zoom-0 CRS px (x, y) | upstream image fraction (fx, fy) | upstream image px | current Rust (fx, fy) | diff |
| --- | --- | --- | --- | --- | --- | --- |
| origin | (0, 0) | (122.0000, 137.6500) | (0.4882, 0.5018) | (403.8616, 381.9796) | (0.4882, 0.5456) | (0.0000, 0.0438) |
| bounds[0] (x=289, z=-293) | (289, -293) | (7.8450, 21.9150) | (-0.0000, -0.0361) | (-0.0000, -27.4717) | (0.0000, 0.0000) | (0.0000, 0.0361) |
| bounds[1] (x=-303, z=244) | (-303, 244) | (241.6850, 234.0300) | (1.0000, 0.9498) | (827.2874, 722.9562) | (1.0000, 1.0000) | (-0.0000, 0.0502) |
| extract "Armored Train" (shared) | (144.986, -147.352) | (64.7305, 79.4460) | (0.2433, 0.2313) | (201.2516, 176.0633) | (0.2433, 0.2712) | (0.0000, 0.0399) |
| extract "D-2" (pmc) | (-121.479065, 172.24913) | (169.9842, 205.6884) | (0.6934, 0.8181) | (573.6219, 622.6883) | (0.6934, 0.8664) | (-0.0000, 0.0483) |
| extract "Bunker Hermetic Door" (shared) | (61.9206619, -190.541931) | (97.5413, 62.3859) | (0.3836, 0.1520) | (317.3309, 115.7078) | (0.3836, 0.1908) | (0.0000, 0.0388) |

**the-lab** — 270°, tiles only (tileSize **175**), fraction = `crsPx0 / 175`

| Position | game (x, z) | zoom-0 CRS px (x, y) | upstream image fraction (fx, fy) | upstream image px | current Rust (fx, fy) | diff |
| --- | --- | --- | --- | --- | --- | --- |
| origin | (0, 0) | (281.2000, 193.7000) | (1.6069, 1.1069) | (281.2000, 193.7000) | (1.6069, 1.1069) | (0.0000, 0.0000) |
| bounds[0] (x=-80, z=-477) | (-80, -477) | (6.9250, 147.7000) | (0.0396, 0.8440) | (6.9250, 147.7000) | (0.0396, 0.8440) | (0.0000, 0.0000) |
| bounds[1] (x=-287, z=-193) | (-287, -193) | (170.2250, 28.6750) | (0.9727, 0.1639) | (170.2250, 28.6750) | (0.9727, 0.1639) | (0.0000, 0.0000) |
| extract "Medical Block Elevator" (shared) | (-112.423, -343.986) | (83.4081, 129.0568) | (0.4766, 0.7375) | (83.4081, 129.0568) | (0.4766, 0.7375) | (0.0000, 0.0000) |
| extract "Cargo Elevator" (shared) | (-112.152, -408.64) | (46.2320, 129.2126) | (0.2642, 0.7384) | (46.2320, 129.2126) | (0.2642, 0.7384) | (0.0000, 0.0000) |
| extract "Main Elevator" (shared) | (-282.304016, -334.896) | (88.6348, 31.3752) | (0.5065, 0.1793) | (88.6348, 31.3752) | (0.5065, 0.1793) | (0.0000, 0.0000) |

(The origin lies outside the tile — Labs is entirely at negative x/z. The
`bounds` rectangle occupies tile fractions x 0.04-0.97, y 0.16-0.84, i.e. it is
the *content* extent inside the tile, which is why the bounds path is wrong here.)

**the-labyrinth** — 270°, tiles only (tileSize 256), fraction = `crsPx0 / 256`

| Position | game (x, z) | zoom-0 CRS px (x, y) | upstream image fraction (fx, fy) | upstream image px | current Rust (fx, fy) | diff |
| --- | --- | --- | --- | --- | --- | --- |
| origin | (0, 0) | (85.5000, 128.0000) | (0.3340, 0.5000) | (85.5000, 128.0000) | (0.3340, 0.5000) | (0.0000, 0.0000) |
| bounds[0] (x=-52, z=-37) | (-52, -37) | (7.2450, 18.0200) | (0.0283, 0.0704) | (7.2450, 18.0200) | (0.0283, 0.0704) | (0.0000, 0.0000) |
| bounds[1] (x=53, z=76) | (53, 76) | (246.2400, 240.0950) | (0.9619, 0.9379) | (246.2400, 240.0950) | (0.9619, 0.9379) | (0.0000, 0.0000) |
| extract "The Way Up" (pmc) | (-7.31, 40.127) | (170.3686, 112.5394) | (0.6655, 0.4396) | (170.3686, 112.5394) | (0.6655, 0.4396) | (0.0000, 0.0000) |
| extract "Ariadne's Path" (pmc) | (19.443, 0.495) | (86.5469, 169.1219) | (0.3381, 0.6606) | (86.5469, 169.1219) | (0.3381, 0.6606) | (0.0000, 0.0000) |

**icebreaker** — 180°, tiles only (tileSize 256), non-uniform transform, fraction = `crsPx0 / 256`

| Position | game (x, z) | zoom-0 CRS px (x, y) | upstream image fraction (fx, fy) | upstream image px | current Rust (fx, fy) | diff |
| --- | --- | --- | --- | --- | --- | --- |
| origin | (0, 0) | (125.0000, 91.0000) | (0.4883, 0.3555) | (125.0000, 91.0000) | (0.5404, 0.4890) | (0.0521, 0.1335) |
| bounds[0] (x=77, z=-64.5) | (77, -64.5) | (-29.0000, -134.7500) | (-0.1133, -0.5264) | (-29.0000, -134.7500) | (0.0000, 0.0000) | (0.1133, 0.5264) |
| bounds[1] (x=-65.5, z=67.4) | (-65.5, 67.4) | (256.0000, 326.9000) | (1.0000, 1.2770) | (256.0000, 326.9000) | (1.0000, 1.0000) | (-0.0000, -0.2770) |

(No extracts for Icebreaker in `maps.ron`. Its `bounds` are Factory's; they do
not describe the tile, so only the transform path is meaningful. A hand check:
`x=0,z=0` -> rotate 180° -> `(0,0)` -> `(2*0+125, -3.5*0+91) = (125, 91)`.)

### Worked derivations (one per case)

- **180° SVG (Customs, ZB-013 `(200.9755, -153.0865)`)**: rotate 180° ->
  `(-200.9755, 153.0865)`. Rotated bounds extents: x ∈ [-698, 372], y ∈ [-237, 307].
  `fx = (-200.9755 + 698)/1070 = 0.4645`, `fy = (307 - 153.0865)/544 = 0.2829`
  (box). Letterbox: box aspect 1070:544 vs viewBox 1062.48:535.17 -> `s =
  boxW/vbW`, vertical slack `544 - 535.17*(1070/1062.48) = 5.0` -> 2.5 each side
  -> `fy_img = (0.2829*544 - 2.5)/538.9 = 0.2809`.
- **90° SVG (Factory, Cellars `(73.894, -29.082)`)**: rotate 90° -> `(29.082,
  73.894)`. Rotated corners of `[[77,-64.5],[-65.5,67.4]]`: x ∈ [-67.4, 64.5],
  y ∈ [-65.5, 77]. `fx = (29.082 + 67.4)/131.9 = 0.7315`, `fy = (77 - 73.894)/142.5
  = 0.0218` (box); letterbox slack in y is 0.1 units -> `0.0215`.
- **270° tiles (Labs, Medical Block Elevator `(-112.423, -343.986)`)**: rotate
  270° -> `(x cos270 - z sin270, x sin270 + z cos270) = (-343.986, 112.423)`.
  `px = 0.575*(-343.986) + 281.2 = 83.408`, `py = -0.575*112.423 + 193.7 =
  129.057`; `/175` -> `(0.4766, 0.7375)`.
- **180° SVG with `svgBounds` (Reserve, D-2 `(-121.479, 172.249)`)**: rotate ->
  `(121.479, -172.249)`. Using `svgBounds` extents x ∈ [-289, 303], y ∈ [-272,
  274]: `fx = (121.479 + 289)/592 = 0.6934`, `fy = (274 + 172.249)/546 = 0.8173`
  (box) -> letterbox -> `0.8181`. Using `bounds` (as the port does): y ∈ [-244,
  293], `fy = (293 + 172.249)/537 = 0.8664`.

## Notes for our implementation (non-normative)

- A faithful port needs one extra bit of data per map: whether the bundled image
  is the tile pyramid or the SVG (`fetch_maps.rs` already knows), and Reserve's
  `svgBounds`. With that, `game_to_display` becomes: tiles -> transform path;
  SVG -> box path over `svgBounds ?? bounds`. The `rotation == 270` predicate can
  go.
- For an oracle tolerance: Labs/Labyrinth vectors are exact to float precision;
  the SVG maps are exact for the *box* fraction and within 0.5 % for the
  letterboxed image fraction. If tests assert the box fraction, use the numbers
  in parentheses under Customs / the "current Rust" column for Streets/Factory
  (they equal upstream's `boxFrac`); if they assert against the bundled PNG, use
  the "upstream image fraction" column and allow ~0.005.
- The 29 non-Icebreaker/Reserve vectors are also a regression net for the
  existing behaviour; the 9 Reserve/Icebreaker vectors document the two bugs.
