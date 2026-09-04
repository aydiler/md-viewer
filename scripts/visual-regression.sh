#!/usr/bin/env bash
#
# Visual regression check for viewport-slice rendering.
#
# The renderer paints the first frame of a document in full (the "bootstrap"
# pass) and every later frame as a viewport slice. A slice that lays out at a
# different column, or that overflows its rect, produces content shifted
# sideways or a blank page below a large block — see docs/LESSONS.md,
# "Viewport slices must reproduce the bootstrap's layout".
#
# This lives here rather than in `cargo test` on purpose: the failure does not
# reproduce in the crate's headless tests. It was chased through five headless
# formulations — a forced-bootstrap reference frame, stored ScrollArea state,
# leftmost-text-on-screen, wheel-driven table-cell comparison, and finally the
# math feature plus md-viewer's own table_max_width/default_width options — and
# every one of them passed on a build that was visibly broken on screen. The
# slice path *is* exercised there (1 bootstrap + 14 slice frames), so the
# harness is not bypassing the code; the shift appears to need md-viewer's real
# font stack, which the crate's test build does not load.
#
# Usage:
#   scripts/visual-regression.sh [path/to/binary] [path/to/document.md]
#
# Requires: Xvfb, xdotool, ImageMagick (import), python3 with Pillow.

set -uo pipefail

BIN="${1:-target/debug/md-viewer}"
DOC="${2:-}"
TAG="visreg"
# Override when :99 is occupied or wedged: MDV_DISPLAY=98 scripts/visual-regression.sh
# (scroll-regression.sh honours the same variable.)
DISPLAY_NUM="${MDV_DISPLAY:-99}"

if [ ! -x "$BIN" ]; then
    echo "error: $BIN is not an executable — build it first (cargo build)" >&2
    exit 2
fi

# A document whose table is taller than the viewport: the shape that leaves the
# viewport with no split point nearby, so the slice must resume from a block far
# above it. That is the case the layout bugs showed up on.
if [ -z "$DOC" ]; then
    DOC="$(mktemp /tmp/mdv-visreg-XXXXXX.md)"
    {
        echo "# Visual regression fixture"
        echo
        echo "Intro paragraph before the table."
        echo
        echo "| Signal | Type | Result | Notes |"
        echo "|---|---|---|---|"
        for i in $(seq 0 79); do
            echo "| signal_$i | type_$i | \$\\frac{$i}{2}\$ | a fairly long table-cell note for row $i that should wrap |"
        done
        echo
        echo "## After the table"
        echo
        echo "Paragraph that must be reachable by scrolling."
        echo
        echo '```rust'
        echo 'fn main() { println!("syntax highlighting"); }'
        echo '```'
        echo
        echo "## Unicode"
        echo
        echo "中文测试 — Thai: สวัสดี"
    } > "$DOC"
    echo "using generated fixture: $DOC"
fi

export DISPLAY=":$DISPLAY_NUM" WINIT_UNIX_BACKEND=x11 WAYLAND_DISPLAY=
# md-viewer persists scroll position and open tabs; isolate so every run starts
# at the top of the document.
export XDG_DATA_HOME="/tmp/mdv-$TAG-data" XDG_CONFIG_HOME="/tmp/mdv-$TAG-config"

cleanup() {
    # Match process NAMES, not the full command line — a `pkill -f` pattern
    # would also match this script and kill the run itself.
    pkill -9 '^md-viewer$' 2>/dev/null
    true
}
trap cleanup EXIT

echo "== starting Xvfb on :$DISPLAY_NUM =="
pkill -9 Xvfb 2>/dev/null
rm -f "/tmp/.X${DISPLAY_NUM}-lock" "/tmp/.X11-unix/X${DISPLAY_NUM}"
# Redirect: a backgrounded process that inherits stdout holds the pipe open,
# so `scripts/visual-regression.sh | tail` would hang forever waiting for EOF.
Xvfb ":$DISPLAY_NUM" -screen 0 1920x1080x24 >/dev/null 2>&1 &
sleep 2
if ! xdpyinfo >/dev/null 2>&1; then
    echo "error: Xvfb is not responding on :$DISPLAY_NUM" >&2
    exit 2
fi

cleanup
rm -rf "$XDG_DATA_HOME" "$XDG_CONFIG_HOME" "/tmp/shot-$TAG-"*.png
mkdir -p "$XDG_DATA_HOME" "$XDG_CONFIG_HOME"

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

for step in 0 1 2 3 4 5 6; do
    if [ "$step" -gt 0 ]; then
        for _ in $(seq 1 10); do xdotool click 5; sleep 0.04; done
        sleep 0.7
    fi
    import -window "$WID" "/tmp/shot-$TAG-$step.png" 2>/dev/null
done

echo "== analysing =="
python3 - "$TAG" <<'PY'
import sys, glob
from PIL import Image

tag = sys.argv[1]
shots = sorted(glob.glob(f"/tmp/shot-{tag}-*.png"))
if not shots:
    print("error: no screenshots captured"); sys.exit(2)

def analyse(path):
    im = Image.open(path).convert("RGB")
    w, h = im.size
    px = im.load()
    bg = px[w - 300, h - 30]                     # empty area = background
    def differs(x, y):
        return sum(abs(a - b) for a, b in zip(px[x, y], bg)) > 30
    # Document pane only: right of the file explorer, left of the outline.
    left, right = 230, w - 220
    content = sum(1 for y in range(50, h - 20, 4) for x in range(left, right, 4)
                  if differs(x, y))
    edges = [next((x for x in range(left, right) if differs(x, y)), None)
             for y in range(60, 220, 10)]
    edges = [e for e in edges if e is not None]
    return content, (min(edges) if edges else None)

results = [analyse(p) for p in shots]
baseline_edge = results[0][1]
failures = []

for path, (content, edge) in zip(shots, results):
    print(f"  {path.split('/')[-1]}: content_px={content:6d} left_edge={edge}")
    # A frame that paints almost nothing means the slice was anchored outside
    # the viewport — the "blank page below the table" failure.
    if content < 500:
        failures.append(f"{path}: only {content} content pixels — frame is effectively blank")
    # Content must stay in the same column at every scroll position.
    if edge is not None and baseline_edge is not None and abs(edge - baseline_edge) > 2:
        failures.append(
            f"{path}: content starts at x={edge}, baseline is x={baseline_edge} "
            f"({abs(edge - baseline_edge)}px shift)")

if failures:
    print("\nFAIL")
    for f in failures:
        print("  " + f)
    sys.exit(1)
print("\nPASS: no blank frames, no horizontal drift across scroll positions")
PY
STATUS=$?

echo "== done (screenshots left in /tmp/shot-$TAG-*.png) =="
exit $STATUS
