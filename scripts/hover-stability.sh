#!/usr/bin/env bash
#
# Stationary-pointer stability check beside a resizable panel divider.
#
# Third companion to scroll-regression.sh and visual-regression.sh, checking a
# failure neither can see: geometry that *alternates* while the pointer does not
# move (issue #139).
#
# Why those two cannot cover it, and why this one can:
#
#   Both existing guards sample settled states — they move, sleep, capture once.
#   That makes them blind to a one-frame artifact (see #140), but an oscillating
#   state never settles, so repeated captures at a *fixed* pointer position do
#   land on different phases. #139 is therefore reachable by capture where #140
#   is not.
#
# What it found: on the build before the fix, exactly one x out of 36 scanned
# cycles through four states with the pointer held still — visible both in the
# painted pixels and in the cursor shape, sampled through XFixes. Its neighbours
# are stable across 20 frames. One pixel wide is why the report reads as
# intermittent, and why an earlier sweep in 14 px steps found nothing: the
# window is narrower than the step.
#
# The failure window is one pixel, so DO NOT coarsen STEP below 1 px thinking it
# saves time; it removes the detection.
#
# Usage:
#   scripts/hover-stability.sh <binary> [tag]
#
# Run it twice — once on the build under test and once on a control — and
# compare. A run that reports zero unstable positions means nothing on its own
# unless the control run reports some; see the "which way it cannot fail" family
# in docs/LESSONS.md.
#
# Requires: Xvfb, xdotool, ImageMagick (import), python3 with Pillow, and
# python-xlib for the cursor sampling (the cursor half is skipped without it).
#
# Detaches nothing: run it under `systemd-run --user --collect` if your caller
# reaps process groups, exactly as the other two guards document.
set -u

BIN="${1:?usage: hover-stability.sh <binary> [tag]}"
TAG="${2:-run}"
DISPLAY_NUM="${MDV_DISPLAY:-95}"
STEP=1
SETTLE="${MDV_SETTLE:-1.5}"
FRAMES="${MDV_FRAMES:-12}"
OUT="${MDV_OUT:-/tmp/mdv-hover-$TAG}"

rm -rf "$OUT"; mkdir -p "$OUT/shots"
FIXTURE="$OUT/fixture"; mkdir -p "$FIXTURE"
# Enough files that the explorer itself scrolls: its vertical scrollbar is the
# one that sits on the explorer/document boundary.
for i in $(seq 1 40); do
    f="$FIXTURE/a-deliberately-long-file-name-$i.md"
    printf '# Document %s\n\n' "$i" > "$f"
    for j in $(seq 1 60); do
        echo "Line $j, long enough that the document scrolls and gets a scrollbar." >> "$f"
    done
done

pkill -f "Xvfb :$DISPLAY_NUM " 2>/dev/null
rm -f "/tmp/.X$DISPLAY_NUM-lock" "/tmp/.X11-unix/X$DISPLAY_NUM"
Xvfb ":$DISPLAY_NUM" -screen 0 1400x900x24 >/dev/null 2>&1 &
XVFB_PID=$!
for _ in $(seq 1 40); do DISPLAY=":$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 && break; sleep 0.25; done
DISPLAY=":$DISPLAY_NUM" xdpyinfo >/dev/null 2>&1 || { echo "Xvfb :$DISPLAY_NUM did not come up"; exit 1; }
export DISPLAY=":$DISPLAY_NUM"

env WINIT_UNIX_BACKEND=x11 WAYLAND_DISPLAY= \
    XDG_DATA_HOME="$OUT/data" XDG_CONFIG_HOME="$OUT/config" \
    setsid "$BIN" --foreground "$FIXTURE/a-deliberately-long-file-name-1.md" \
    >"$OUT/app.log" 2>&1 &
APP_PID=$!
for _ in $(seq 1 60); do
    WID=$(xdotool search --name "Markdown Viewer" 2>/dev/null | head -1)
    [ -n "${WID:-}" ] && break
    sleep 0.5
done
if [ -z "${WID:-}" ]; then
    echo "FAIL: no window appeared"; tail -5 "$OUT/app.log"
    kill "$APP_PID" "$XVFB_PID" 2>/dev/null; exit 1
fi
sleep 3

shot()  { import -window "$WID" "$OUT/shots/$1.png" 2>/dev/null; }
hold()  { xdotool mousemove --sync "$1" "$2"; }

# Narrow the centre pane by widening the explorer, which is the condition the
# report names. The divider starts near x=213 at this window size.
hold 213 400; sleep 0.4
xdotool mousedown 1; sleep 0.3
for x in $(seq 220 15 430); do xdotool mousemove --sync "$x" 400; sleep 0.05; done
sleep 0.4; xdotool mouseup 1; sleep 2
hold 700 400; sleep 1.5

: > "$OUT/cursor.txt"
for x in $(seq 405 "$STEP" 440); do
    hold "$x" 400
    sleep "$SETTLE"
    for f in $(seq -w 1 "$FRAMES"); do
        shot "x${x}_f${f}"
        printf '%s %s ' "$x" \
            "$(python3 "$(dirname "$0")/hover-cursor-hash.py" ":$DISPLAY_NUM" 2>/dev/null | awk '{print $1}')" \
            >> "$OUT/cursor.txt"
        sleep 0.15
    done
    echo >> "$OUT/cursor.txt"
done

kill -TERM "$APP_PID" 2>/dev/null; sleep 1; kill "$XVFB_PID" 2>/dev/null

python3 - "$OUT" "$TAG" <<'PY'
from PIL import Image
import hashlib, glob, re, sys, os
out, tag = sys.argv[1], sys.argv[2]
BOX = (400, 140, 470, 700)   # the divider column and the scrollbar beside it
by_x = {}
for p in glob.glob(f"{out}/shots/x*_f*.png"):
    m = re.match(r".*/x(\d+)_f(\d+)\.png", p)
    by_x.setdefault(int(m.group(1)), []).append(
        hashlib.sha1(Image.open(p).convert("RGB").crop(BOX).tobytes()).hexdigest()[:8])
pixel = {x: len(set(v)) for x, v in by_x.items() if len(set(v)) > 1}
cursor = {}
cfile = f"{out}/cursor.txt"
if os.path.exists(cfile):
    for line in open(cfile):
        q = line.split()
        if len(q) < 4:
            continue
        if len(set(q[1::2])) > 1:
            cursor[int(q[0])] = len(set(q[1::2]))
print(f"[{tag}] {len(by_x)} positions x {len(next(iter(by_x.values()), []))} frames, pointer held still")
print(f"[{tag}] pixels unstable at:  {dict(sorted(pixel.items())) or 'none'}")
print(f"[{tag}] cursor unstable at:  {dict(sorted(cursor.items())) or 'none'}")
if pixel or cursor:
    print(f"[{tag}] FAIL: geometry alternates with an unmoving pointer")
    sys.exit(1)
print(f"[{tag}] PASS")
PY
