# Prototype — categorised Overlays sidebar (#28)

Throwaway. Three variants of the Overlays section, mounted in the real sidebar.

Run: `nix develop -c cargo run --bin tarkov-map` on this branch.
Switch variants with the yellow bar at the bottom of the map or ← / →; `G` toggles glyphs.
Screenshots: `TARKOV_MAP_PROTO_SHOTS=<dir> cargo run --bin tarkov-map` writes one PNG per variant × glyphs and exits.

| | glyphs on | glyphs off |
|---|---|---|
| A — Eyebrow headers (#27 as decided) | variant-A-glyphs-on.png | variant-A-glyphs-off.png |
| B — Chips | variant-B-glyphs-on.png | variant-B-glyphs-off.png |
| C — Collapsing headers | variant-C-glyphs-on.png | variant-C-glyphs-off.png |

Colours for the five new overlays are placeholders, not the visual-language decision.
