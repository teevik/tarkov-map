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
A named group of Overlays that share a collapsible heading in the sidebar. Purely presentational: a category has no visibility of its own — collapsing it hides its toggles in the sidebar, not the Overlays on the Map — and its only state is whether it is open.
_Avoid_: Section (the sidebar's top-level headings such as "Map" and "Overlays"), group, layer group

**Trail**:
The recent Player Positions on the current Map, drawn as an Overlay behind the newest one. A Trail belongs to one Map and is discarded when another Map is selected.
_Avoid_: History, breadcrumbs, path

**Map Suggestion**:
An offer to switch Maps, made when a Player Position falls outside the selected Map but inside exactly one other Map. The player accepts or dismisses it; the app never switches Maps on its own.
_Avoid_: Auto-detect, auto-switch

**Notification**:
A message the app records for the player about something they should know — an error, an available update, "already up to date" — with a severity of Info, Warning or Error. The toast is the drawing of a Notification, not the Notification itself.
_Avoid_: Toast, alert, message

## Not modelled

- **Floors / layers**: tarkov.dev exposes per-floor tile sets with height extents. This project renders the Main Floor only; there is no floor selector and no height-based floor matching. Do not reintroduce layer support "for completeness".
