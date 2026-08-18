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
The player's game-space coordinates, height and heading at one moment, together with when it was taken; parsed from an in-raid screenshot filename. Height (Y) is displayed but never used to choose what is drawn.
_Avoid_: Fix, marker (the marker is the drawing of a Player Position)

**Position Source**:
Where Player Positions come from: the screenshots folder in normal use, or the demo walker when running without the game. The app tracks exactly one Position Source at a time.
_Avoid_: Watcher (the screenshot watcher is one Position Source), tracker

**Freshness**:
Whether the newest Player Position is recent enough to trust: Live or Stale, judged by its age against a fixed threshold. Freshness is read off the Player Position, never stored or configured.
_Avoid_: Staleness levels, timeout, live mode

**Overlay**:
One toggleable set of markers or areas drawn over a Map's image (e.g. PMC Extracts, Labels, Minefields, the Player Position marker). Each Overlay is independently shown or hidden, and that choice is remembered across Maps and restarts; an Overlay with nothing to draw on the current Map is not offered in the sidebar at all (only the Player Position marker is always offered). The player never orders or stacks Overlays — the app always draws area Overlays beneath marker Overlays.
_Avoid_: Layer (reserved for tarkov.dev's per-floor tile sets, which are not modelled)

**Overlay Category**:
A named group of Overlays that share a collapsible heading in the sidebar. Purely presentational: a category has no visibility of its own — collapsing it hides its toggles in the sidebar, not the Overlays on the Map — and its only state is whether it is open. A category none of whose Overlays are offered on the current Map is not shown.
_Avoid_: Section (the sidebar's top-level headings such as "Map" and "Overlays"), group, layer group

**Boss Spawn**:
A position on a Map where one or more of the named mobs tarkov.dev lists as bosses — including Raiders, Rogues and faction squads such as AF and Black Div. — may appear, each with its map-wide spawn chance. Drawn as the "Bosses" Overlay; escorts are not Boss Spawns.
_Avoid_: Boss zone, boss location, spawn location (tarkov.dev's zone grouping, which is not modelled)

**Sniper Zone**:
An area of a Map covered by a stationary marksman, as outlined by tarkov.dev; drawn as its outline on the Map. Drawn as the "Sniper zones" Overlay.
_Avoid_: Sniper (the mob), sniper hazard, hazard zone

**Minefield**:
An area of a Map that kills on entry — a mined field, a single mine strip, or Labyrinth's generic hazard patches — as outlined by tarkov.dev; drawn as its outline on the Map, or as a small marker when the outline is too small to see. Drawn as the "Minefields" Overlay.
_Avoid_: Hazard (tarkov.dev's umbrella type, which also covers Sniper Zones), mine, landmine

**Transit**:
A point on a Map from which a raid continues on another Map, labelled with the destination Map's name. Drawn as a marker like an Extract; its entry conditions and outline are not modelled.
_Avoid_: Transfer, portal, exit (an Extract leaves the raid; a Transit continues it)

**Switch**:
A lever, button or console on a Map that the player can operate — powering an elevator, opening a door, disabling a trap or unlocking an Extract — as listed by tarkov.dev, drawn as a marker labelled with its name in the "Switches" Overlay. What a Switch controls is not modelled beyond what its name says.
_Avoid_: Lever, button, console (kinds of Switch), switch-controlled extract (an Extract is drawn the same whether or not a Switch gates it)

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
