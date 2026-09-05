#!/usr/bin/env bash
#
# Deep-scroll regression check: blank frames and stuck scrolling.
#
# Companion to visual-regression.sh, which checks that a viewport slice lays
# out in the same column as the bootstrap pass. This script checks a different
# failure: a slice that anchors outside the viewport and paints *nothing*, so
# scrolling appears to skip whole sections of the document (issue #121).
#
# Why a second script rather than more assertions in the first one — both gaps
# were found the hard way while confirming #121 on main @ 7b8f53b, a build that
# visual-regression.sh passes:
#
#   1. Depth. visual-regression.sh scrolls 7 steps of 10 wheel clicks. On a
#      1200-line document with several 30-50 row tables the first blank frame
#      only appears around step 14, so the shorter walk never reaches it. This
#      script defaults to 60 steps and reports every frame.
#
#   2. The left-edge metric does not generalize. visual-regression.sh flags a
#      shift when the leftmost content pixel moves. That works on its own
#      fixture (flush-left headings and one table) but false-positives on real
#      documents: an indented list item is legitimately further right, which
#      reported a bogus "26px shift" on the wrench asset_reference.md. This
#      script keys off painted-content *volume* and frame-to-frame difference
#      instead, which does not depend on document shape.
#
# What it asserts:
#   - No frame paints an effectively empty document pane (the #121 symptom).
#   - Scrolling is not stuck before the document bottom is reached (a run of
#     identical frames is only accepted once the end has been reached, which is
#     the legitimate at-the-bottom case).
#
# This guard earned its keep on the first run: against the build of the day it
# found a blank pane at the fixture's Section 6/7 table boundary that #113 had
# not addressed and that reproduced identically on 7b8f53b. That became #125,
# and #114 fixed it. The guard is green on main as of 7a6346c.
#
# When it fails, check the control before believing the fix: re-run against the
# previous build. A PASS only means something if the same harness still reports
# FAIL on a build known to be broken.
#
# Usage:
#   scripts/scroll-regression.sh [binary] [document.md] [steps] [clicks_per_step]
#
# Defaults: target/debug/md-viewer, a generated table-heavy fixture, 150, 3.
# Exit 0 = pass, 1 = regression found, 2 = harness problem.
#
# Requires: Xvfb, xdotool, ImageMagick (import), python3 with Pillow.

set -uo pipefail

BIN="${1:-target/debug/md-viewer}"
DOC="${2:-}"
STEPS="${3:-150}"
CLICKS="${4:-3}"
TAG="scrollreg"
# Override when :99 is occupied or wedged: MDV_DISPLAY=98 scripts/scroll-regression.sh
DISPLAY_NUM="${MDV_DISPLAY:-99}"

if [ ! -x "$BIN" ]; then
    echo "error: $BIN is not an executable — build it first (cargo build)" >&2
    exit 2
fi

# A document with several tables taller than the viewport, separated by prose.
# Tables are the shape that triggered #121: `table()` consumes the table's End
# event, so a split point after it has to be recorded from the Start event —
# get that wrong and the slice after a table has no anchor and paints nothing.
if [ -z "$DOC" ]; then
    DOC="$(mktemp /tmp/mdv-scrollreg-XXXXXX.md)"
    # Shaped after the document from #121: twenty tables between 4 and 51 rows,
    # cell text of varying length so row heights differ, and a nested index list
    # at the top. Both properties are load-bearing — a fixture of five identical
    # 40-row tables reaches the bottom three times sooner and never goes blank
    # on a build that is visibly broken. See docs/LESSONS.md.
    SIZES=(6 23 11 4 51 9 5 7 30 4 38 18 4 12 26 4 33 8 15 44)
    {
        echo "# Scroll regression fixture"
        echo
        echo "## Index"
        echo
        for group in Assets Globals Levels Miscellaneous; do
            echo "- $group"
            for sub in 1 2 3 4 5 6; do echo "  - ${group}Entry$sub"; done
        done
        echo
        n=0
        for size in "${SIZES[@]}"; do
            n=$((n + 1))
            echo "## Section $n"
            echo
            echo "### Attributes"
            echo
            echo "A short prose line introducing section $n."
            echo
            echo "| Name | Description | Type | Required | Games |"
            echo "| - | - | - | - | - |"
            for i in $(seq 1 "$size"); do
                case $((i % 4)) in
                    0) desc="*Not yet documented.*" ;;
                    1) desc="A considerably longer description cell for row $i that has to wrap across several lines inside its column, which makes this row taller than its neighbours." ;;
                    2) desc="Short." ;;
                    3) desc="A medium length description for row $i covering roughly two lines of text." ;;
                esac
                echo "| entry_${n}_$i | $desc | Collection | Yes | RAC/GC/UYA/DL |"
            done
            echo
            echo "### Children"
            echo
            echo "No children."
            echo
        done
        echo "## End marker"
        echo
        echo "SCROLL_REGRESSION_END_MARKER"
    } > "$DOC"
    echo "using generated fixture: $DOC"
fi

export DISPLAY=":$DISPLAY_NUM" WINIT_UNIX_BACKEND=x11 WAYLAND_DISPLAY=
# md-viewer persists scroll position and open tabs; isolate so every run starts
# at the top of the document.
export XDG_DATA_HOME="/tmp/mdv-$TAG-data" XDG_CONFIG_HOME="/tmp/mdv-$TAG-config"

FRAMES="/tmp/frames-$TAG"

APP_PID=""
XVFB_PID=""
# Safe to call mid-run: touches only the application.
kill_app() {
    # Kill by recorded PID. Matching by name does not work reliably: Linux
    # truncates a process's comm to 15 characters, so `pkill -x` misses any
    # binary with a longer name (a copy called `mdv-main-7a6346c` shows up as
    # `mdv-main-7a6346`), and `pkill -f` would match this script's own command
    # line and kill the run itself.
    [ -n "$APP_PID" ] && kill -9 "$APP_PID" 2>/dev/null
    true
}

# EXIT trap only — never call this while the run still needs the display.
cleanup() {
    kill_app
    # Kill the Xvfb this run started, by PID. Leaving it behind means the
    # next run's blunt `pkill -9 Xvfb` has to clean up after us — which
    # also kills any unrelated Xvfb on the machine, and races with a
    # concurrent guard run.
    [ -n "$XVFB_PID" ] && kill -9 "$XVFB_PID" 2>/dev/null
    true
}
trap cleanup EXIT

echo "== starting Xvfb on :$DISPLAY_NUM =="
# Kill only an Xvfb on *our* display, never every Xvfb on the machine:
# this box runs a systemd user unit `xvfb99.service` (Restart=always),
# and a blanket `pkill -9 Xvfb` takes it down on every guard run — which
# is both rude and a source of races when two runs overlap. `pgrep -f`
# is safe here because this pattern cannot match the script's own
# command line.
for pid in $(pgrep -f "Xvfb :$DISPLAY_NUM " 2>/dev/null); do
    kill -9 "$pid" 2>/dev/null
done
rm -f "/tmp/.X${DISPLAY_NUM}-lock" "/tmp/.X11-unix/X${DISPLAY_NUM}"
# Redirect: a backgrounded process that inherits stdout holds the pipe open,
# so `scripts/scroll-regression.sh | tail` would hang forever waiting for EOF.
Xvfb ":$DISPLAY_NUM" -screen 0 1920x1080x24 >/dev/null 2>&1 &
XVFB_PID=$!
sleep 2
if ! xdpyinfo >/dev/null 2>&1; then
    echo "error: Xvfb is not responding on :$DISPLAY_NUM" >&2
    exit 2
fi

kill_app
rm -rf "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$FRAMES"
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "$FRAMES"

# Screenshots of an obscured X11 window return the *overlapping* window's
# pixels, so a leftover instance silently corrupts the comparison. Insist on
# exactly one window.
STALE=$(xdotool search --name "Markdown Viewer" 2>/dev/null | wc -l)
if [ "$STALE" -ne 0 ]; then
    echo "error: $STALE stale md-viewer windows remain on :$DISPLAY_NUM" >&2
    exit 2
fi

echo "== launching $BIN =="
setsid "$BIN" --foreground "$DOC" </dev/null >"/tmp/mdv-$TAG.log" 2>&1 &
APP_PID=$!
sleep 5

WINDOWS=$(xdotool search --name "Markdown Viewer" 2>/dev/null)
COUNT=$(echo "$WINDOWS" | grep -c .)
if [ "$COUNT" -ne 1 ]; then
    echo "error: expected exactly 1 window, found $COUNT" >&2
    exit 2
fi
WID=$(echo "$WINDOWS" | head -1)
xdotool windowsize "$WID" 1200 800
sleep 2
xdotool mousemove 600 400
sleep 0.5

echo "== capturing $((STEPS + 1)) frames ($CLICKS wheel clicks per step) =="
for step in $(seq 0 "$STEPS"); do
    if [ "$step" -gt 0 ]; then
        for _ in $(seq 1 "$CLICKS"); do xdotool click 5; sleep 0.03; done
        sleep 0.45
    fi
    import -window "$WID" "$FRAMES/$(printf 'f%03d' "$step").png" 2>/dev/null
done

echo "== analysing =="
python3 - "$FRAMES" <<'PY'
import sys, glob
from PIL import Image, ImageChops

frames = sorted(glob.glob(f"{sys.argv[1]}/f*.png"))
if not frames:
    print("error: no screenshots captured"); sys.exit(2)

# Document pane only: right of the file explorer, left of the outline. Both
# side panels keep painting even when the document pane goes blank, so a
# whole-window measurement would hide exactly the failure we are looking for.
LEFT, TOP, RIGHT_INSET = 225, 45, 215

def pane(path):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    return im.crop((LEFT, TOP, w - RIGHT_INSET, h))

rows, prev = [], None
for path in frames:
    pn = pane(path)
    px = pn.load()
    w, h = pn.size
    bg = px[w - 40, h - 12]
    content = sum(1 for y in range(0, h, 4) for x in range(0, w, 4)
                  if sum(abs(a - b) for a, b in zip(px[x, y], bg)) > 30)
    if prev is None:
        diff = None
    else:
        d = ImageChops.difference(pn, prev).convert("L").load()
        diff = sum(1 for y in range(0, h, 4) for x in range(0, w, 4) if d[x, y] > 24)
    rows.append((path, content, diff))
    prev = pn

# A run of identical frames at the tail is the document bottom, which is
# correct. The same run in the middle means scrolling stopped advancing.
tail_static = 0
for _, _, diff in reversed(rows):
    if diff is not None and diff < 40:
        tail_static += 1
    else:
        break
bottom_from = len(rows) - tail_static

blank, stuck = [], []
for idx, (path, content, diff) in enumerate(rows):
    name = path.split("/")[-1]
    note = ""
    if content < 300:
        note += "BLANK "
        blank.append(name)
    if diff is not None and diff < 40 and idx < bottom_from:
        note += "STUCK "
        stuck.append(name)
    print(f"  {name}: content_px={content:6d} diff={diff if diff is not None else '-':>6} {note}")

print()
if blank:
    print(f"FAIL: {len(blank)} frame(s) painted an empty document pane: {', '.join(blank)}")
if stuck:
    print(f"FAIL: {len(stuck)} frame(s) did not advance before the document bottom: {', '.join(stuck)}")
if blank or stuck:
    sys.exit(1)
print(f"PASS: no blank frames, scrolling advanced until the bottom "
      f"(reached at frame {bottom_from - 1} of {len(rows) - 1})")
PY
STATUS=$?

echo "== done (frames left in $FRAMES) =="
exit $STATUS
