#!/usr/bin/env bash
# Screenshot pass for prototype #58 under Hyprland (0.56 Lua API): floats the app
# window once at 1100×900, then lets TARKOV_MAP_PROTO_SHOTS drive the capture.
set -u
out=${1:-/tmp/proto58/shots}
rm -rf "$out"
TARKOV_MAP_PROTO_SHOTS="$out" nix develop -c cargo run --bin tarkov-map >/tmp/proto58-run.log 2>&1 &
runner=$!
JQ="nix shell nixpkgs#jq -c jq"
state() { hyprctl clients -j | $JQ -c '.[] | select(.class=="tarkov-map") | {size,floating}'; }
for _ in $(seq 1 120); do
  [ "$(hyprctl clients -j | grep -c '"class": "tarkov-map"')" -gt 0 ] && break
  sleep 0.5
done
sleep 1.5
for _ in 1 2 3; do
  case "$(state)" in *'"floating":true'*) break;; esac
  hyprctl eval 'local w=hl.get_windows({class="tarkov-map"})[1]; hl.dispatch(hl.dsp.window.float(w))' >/dev/null
  sleep 1.5
done
for _ in 1 2 3; do
  case "$(state)" in *'[1100,900]'*) break;; esac
  hyprctl eval 'local w=hl.get_windows({class="tarkov-map"})[1]; hl.dispatch(hl.dsp.window.resize({x=1100,y=900,window=w}))' >/dev/null
  sleep 1.5
done
echo "window: $(state)"
wait $runner
echo "shots: $(ls "$out" | wc -l)"
