# Tarkov Map

Native desktop viewer for Escape from Tarkov maps that places the player on the map from in-raid screenshots. One image per map; floors are deliberately not modelled.

## Language

**Map**:
One Escape from Tarkov location as published by tarkov.dev's interactive maps, rendered as a single image with its overlays.
_Avoid_: Level, location, layer

**Main Floor**:
The base image tarkov.dev shows by default for a Map; the only floor this app fetches, bundles, or renders. Not necessarily a literal ground floor (Icebreaker's default view is its Infirmary deck).
_Avoid_: Layer, base layer, default layer

**Player Position**:
The player's game-space coordinates and facing, parsed from an in-raid screenshot filename. Height (Y) is displayed but never used to choose what is drawn.
_Avoid_: Fix, marker (the marker is the drawing of a Player Position)

**Overlay**:
One toggleable set of markers drawn over a Map's image (e.g. PMC Extracts, Labels, the Player Position marker). Each Overlay is independently shown or hidden; there is no ordering or stacking between them.
_Avoid_: Layer (reserved for tarkov.dev's per-floor tile sets, which are not modelled)

**Overlay Category**:
A named group of Overlays that share a heading in the sidebar. Purely presentational: a category has no visibility of its own and no behaviour beyond grouping its Overlays.
_Avoid_: Section (the sidebar's top-level headings such as "Map" and "Overlays"), group, layer group

## Not modelled

- **Floors / layers**: tarkov.dev exposes per-floor tile sets with height extents. This project renders the Main Floor only; there is no floor selector and no height-based floor matching. Do not reintroduce layer support "for completeness".
