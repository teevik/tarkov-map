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

**Bounds**:
A Map's playable extent in game space — the box a Player Position is tested against to say whether it lies on that Map (the test Map Suggestion relies on). Bounds describe the ground, not the picture: they need not coincide with the image's edges.
_Avoid_: Extent, image bounds, svg bounds (a Projection detail), viewport (what the player is looking at on screen)

**Projection**:
How a Map's game coordinates land on its Main Floor image — one mapping per Map, fixed when the image is fetched, that turns a Player Position or marker position into a spot on the picture (and a heading into a direction on it). There is one Projection per Map, never per floor.
_Avoid_: Transform (the maths inside a Projection), coordinate rotation, CRS

**Overlay**:
One toggleable set of markers or areas drawn over a Map's image (e.g. PMC Extracts, Labels, Minefields, the Player Position marker). Each Overlay is independently shown or hidden, and that choice is remembered across Maps and restarts; an Overlay with nothing to draw on the current Map is not offered in the sidebar at all (only the Player Position marker is always offered). The player never orders or stacks Overlays — the app always draws area Overlays beneath marker Overlays.
_Avoid_: Layer (reserved for tarkov.dev's per-floor tile sets, which are not modelled)

**Label**:
Any text drawn on a Map: a place name from the "Labels" Overlay, or the name beside a marker (an Extract, Transit, BTR Stop, Switch or Boss Spawn). At any zoom a Label is drawn only if it does not overlap a Label already drawn; Labels are placed in a fixed priority order — Extracts, Transits, BTR Stops, Switches, Boss Spawns, then place names (larger first) — so the important ones survive when a Map is crowded, a culled Label is simply not drawn (its marker still is), and the same Labels survive wherever the Viewport is panned; zooming in makes room and reveals more. A Label that stacks several names (a Boss Spawn's Mobs, clustered Switches) is drawn whole or not at all.
_Avoid_: Caption, text, annotation, tooltip (nothing is revealed on hover), label culling as a setting (it is never off)

**Overlay Category**:
A named group of Overlays that share a collapsible heading in the sidebar. Purely presentational: a category has no visibility of its own — collapsing it hides its toggles in the sidebar, not the Overlays on the Map — and its only state is whether it is open. A category none of whose Overlays are offered on the current Map is not shown.
_Avoid_: Section (the sidebar's top-level headings such as "Map" and "Overlays"), group, layer group

**Mob**:
One named enemy that tarkov.dev lists as a boss on a Map — a real boss (Reshala, Killa), a squad (Raider, Rogue, AF, Black Div.) or a cultist — identified by its translated name. Each Mob present on the current Map is offered as its own Overlay in the Spawns category; escorts and guards are not Mobs.
_Avoid_: Boss (only some Mobs are bosses), bot, faction, `mob` key (tarkov.dev's raw id, several of which map to one Mob)

**Boss Spawn**:
A position on a Map where one or more Mobs may appear, each with its map-wide spawn chance. Drawn as one marker listing every Mob whose Overlay is on; hidden when none is. The marker always draws; its Label follows the general Label rule.
_Avoid_: Boss zone, boss location, spawn location (tarkov.dev's zone grouping, which is not modelled), "Bosses" Overlay (there is none — each Mob is its own Overlay)

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
A lever, button or console on a Map that the player can operate — powering an elevator, opening a door, disabling a trap or unlocking an Extract — as listed by tarkov.dev, drawn as a marker labelled with its name in the "Switches" Overlay; Switches within a few metres of each other share one marker with their names stacked (one Label). What a Switch controls is not modelled beyond what its name says.
_Avoid_: Lever, button, console (kinds of Switch), switch-controlled extract (an Extract is drawn the same whether or not a Switch gates it)

**BTR Stop**:
A named point on a Map where the BTR armoured taxi halts to pick up or drop off players, as listed by tarkov.dev; drawn as a marker labelled with its stop name in the "BTR stops" Overlay. Only Maps the BTR services have any.
_Avoid_: BTR route (not modelled), taxi stop, checkpoint (one stop happens to be named so)

**Trail**:
The recent Player Positions on the current Map, drawn as an Overlay behind the newest one. A Trail belongs to one Map and is discarded when another Map is selected.
_Avoid_: History, breadcrumbs, path

**Map Suggestion**:
An offer to switch Maps, made when a Player Position falls outside the selected Map but inside exactly one other Map. The player accepts or dismisses it; the app never switches Maps on its own.
_Avoid_: Auto-detect, auto-switch

**Viewport**:
The part of the selected Map's image the player is looking at — a centre on the image and a zoom, where the smallest zoom fits the whole Map on screen. Only the player moves the Viewport (pan, zoom, reset, centre on the Player Position once); the app never moves it on its own, and it resets when another Map is selected.
_Avoid_: Camera, view, follow mode (deliberately not offered)

**Map Transition**:
The changeover from the previously shown Map image to the newly selected one: the newly selected image loads (a placeholder appears only if it takes noticeably long), then fades in over the outgoing image, which is kept until the fade completes.
_Avoid_: Crossfade (the drawing of a Map Transition), loading state

**Notification**:
A message the app records for the player about something they should know — an error, an available update, "already up to date" — with a severity of Info, Warning or Error. The toast is the drawing of a Notification, not the Notification itself.
_Avoid_: Toast, alert, message

## Not modelled

- **Floors / layers**: tarkov.dev exposes per-floor tile sets with height extents. This project renders the Main Floor only; there is no floor selector and no height-based floor matching. Do not reintroduce layer support "for completeness".
