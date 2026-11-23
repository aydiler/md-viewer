#!/usr/bin/env bash
# Start a built snap's md-viewer in its base (core22) and check that it renders.
#
#   snap-smoke.sh <file.snap>
#
# The snap job built the snap but never ran it, and two releases shipped snaps
# that failed only when started: 0.3.9-0.3.11 aborted on X11 because winit
# dlopens libraries the snap no longer staged (#272), and 0.3.11 aborted when
# fontconfig found no fonts (#279). This runs the snap's binary under bubblewrap
# with the base as `/`, the snap mounted at /snap/<name>/x1 and the environment
# from its own meta/snap.yaml, on a private Xvfb, twice: with the host's fonts
# and with none. Each run passes if the app is still running after
# SNAP_SMOKE_WAIT seconds, printed no panic, and the screen is not blank.
# Recipe and background: docs/devlog/113-snap-x11-libs.md, 117-snap-smoke-test.md.
#
# Environment:
#   SNAP_SMOKE_BASE  unpacked base directory or a base .snap (default: download
#                    the base named in meta/snap.yaml from the Snap Store)
#   SNAP_SMOKE_WAIT  seconds to let the app start (default 10)
#   SNAP_SMOKE_OUT   directory to copy each run's log and screenshot into
#                    (for a CI artifact)
#
# Needs bwrap, Xvfb, ImageMagick's import/identify, unsquashfs, curl and
# python3 with PyYAML. Under CI (CI=true) it installs them and lifts Ubuntu
# 24.04's AppArmor restriction on unprivileged user namespaces, which bwrap
# needs.
set -euo pipefail

started=$(date +%s)
snap_file=$(readlink -f "${1:?usage: snap-smoke.sh <file.snap>}")
wait_s=${SNAP_SMOKE_WAIT:-10}
work=$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/snap-smoke.XXXXXX")
xvfb_pid=""

cleanup() {
    if [ -n "$xvfb_pid" ]; then
        kill "$xvfb_pid" 2>/dev/null || true
    fi
    if [ -n "$work" ] && [ -d "$work" ]; then
        rm -rf -- "$work"
    fi
}
trap cleanup EXIT

retry() {
    local attempts=$1 i; shift
    for ((i = 1; i <= attempts; i++)); do
        if "$@"; then return 0; fi
        if ((i < attempts)); then
            echo "::warning::attempt $i/$attempts failed: $*; retrying in $((i * 15)) s"
            sleep $((i * 15))
        fi
    done
    echo "::error::failed after $attempts attempts: $*"
    return 1
}

ci_setup() {
    local apt=(-o Acquire::Retries=3 -o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20)
    retry 3 sudo apt-get update -q "${apt[@]}"
    retry 3 sudo apt-get install -y -q "${apt[@]}" \
        bubblewrap xvfb imagemagick squashfs-tools python3-yaml curl
    local knob=/proc/sys/kernel/apparmor_restrict_unprivileged_userns
    if [ -r "$knob" ] && [ "$(cat "$knob")" = 1 ]; then
        sudo sysctl -q -w kernel.apparmor_restrict_unprivileged_userns=0
    fi
}

# unpack <file.squashfs> <dir> <file that must exist afterwards>
# As a normal user unsquashfs cannot create the base's device nodes or write
# its security xattrs and exits 2 ("non-fatal errors"). bwrap mounts its own
# /dev, so those do not matter; exit 1 (fatal) and a missing key file do.
unpack() {
    local rc=0
    unsquashfs -q -no-xattrs -d "$2" "$1" >/dev/null 2>"$work/unsquashfs.err" || rc=$?
    if [ "$rc" -ge 1 ] && [ "$rc" -ne 2 ] || [ ! -e "$2/$3" ]; then
        echo "::error::could not unpack $(basename "$1") (unsquashfs exit $rc)"
        tail -n 5 "$work/unsquashfs.err"
        return 1
    fi
}

echo "[1/5] Preparing tools"
if [ "${CI:-}" = true ]; then
    ci_setup
fi
for tool in bwrap Xvfb import identify unsquashfs curl python3; do
    command -v "$tool" >/dev/null || { echo "::error::$tool is not installed"; exit 1; }
done

echo "[2/5] Unpacking $(basename "$snap_file")"
unpack "$snap_file" "$work/snap" meta/snap.yaml
read -r name base < <(python3 - "$work/snap/meta/snap.yaml" <<'PY'
import sys, yaml
meta = yaml.safe_load(open(sys.argv[1]))
print(meta["name"], meta.get("base", "core"))
PY
)
echo "snap $name, base $base"

echo "[3/5] Getting the $base base"
if [ -n "${SNAP_SMOKE_BASE:-}" ] && [ -d "$SNAP_SMOKE_BASE" ]; then
    root=$(readlink -f "$SNAP_SMOKE_BASE")
else
    if [ -n "${SNAP_SMOKE_BASE:-}" ]; then
        base_snap=$(readlink -f "$SNAP_SMOKE_BASE")
    else
        base_snap="$work/$base.snap"
        # The store drops requests now and then (devlog 078), so each call is
        # retried; it writes to a file so retry's messages stay out of the data.
        retry 4 curl -fsS -o "$work/base-info.json" -H 'Snap-Device-Series: 16' \
            "https://api.snapcraft.io/v2/snaps/info/$base?architecture=amd64"
        url=$(python3 - "$work/base-info.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
print(next(c["download"]["url"] for c in d["channel-map"]
           if c["channel"]["name"] == "stable" and c["channel"]["architecture"] == "amd64"))
PY
)
        retry 4 curl -fsSL -o "$base_snap" "$url"
    fi
    unpack "$base_snap" "$work/base" bin/sh
    root="$work/base"
fi

# A launcher that sets the environment the way snapd does: the snap's global
# environment, then the app's, each value expanded by the base's own shell.
mkdir -p "$work/run" "$work/home"
python3 - "$work/snap/meta/snap.yaml" "$name" > "$work/run/launch.sh" <<'PY'
import shlex, sys, yaml
meta = yaml.safe_load(open(sys.argv[1]))
app = meta["apps"][sys.argv[2]]
print("#!/bin/sh")
print("set -e")
for scope in (meta.get("environment") or {}, app.get("environment") or {}):
    for key, value in scope.items():
        # Values are shell words with $SNAP / ${VAR:+...} expansions, as in snapd.
        print(f'export {key}="{value}"')
print(f'exec "$SNAP/{app["command"]}" --foreground /tmp/run/smoke.md')
PY
cat > "$work/run/smoke.md" <<'MD'
# Snap smoke test

If this renders, the snap started.

```rust
fn main() {}
```
MD

echo "[4/5] Starting Xvfb"
# A free display well above the ones desktops use; -displayfd would take :0.
display=""
for n in $(seq 90 119); do
    if [ ! -e "/tmp/.X$n-lock" ] && [ ! -e "/tmp/.X11-unix/X$n" ]; then
        Xvfb ":$n" -screen 0 1024x640x24 -nolisten tcp </dev/null >/dev/null 2>&1 &
        xvfb_pid=$!
        for _ in $(seq 50); do
            [ -e "/tmp/.X11-unix/X$n" ] && break
            sleep 0.1
        done
        if [ -e "/tmp/.X11-unix/X$n" ] && kill -0 "$xvfb_pid" 2>/dev/null; then
            display=":$n"
            break
        fi
        kill "$xvfb_pid" 2>/dev/null || true
        xvfb_pid=""
    fi
done
[ -n "$display" ] || { echo "::error::Xvfb did not start on any display from :90 to :119"; exit 1; }
echo "Xvfb on $display"

# run_once <label> <fonts: host|none>
run_once() {
    local label=$1 fonts=$2 font_args app_pid alive=yes colors failed=0
    if [ "$fonts" = none ]; then
        font_args=(--tmpfs /usr/share/fonts)
    else
        font_args=(--ro-bind /usr/share/fonts /usr/share/fonts)
    fi
    local snap_dir="/snap/$name/x1"
    bwrap --ro-bind "$root" / \
        --tmpfs /snap --tmpfs /home --tmpfs /tmp --dev /dev --proc /proc \
        --ro-bind "$work/snap" "$snap_dir" \
        --bind /tmp/.X11-unix /tmp/.X11-unix \
        "${font_args[@]}" \
        --ro-bind "$work/run" /tmp/run \
        --bind "$work/home" /home/u \
        --unshare-pid --die-with-parent --clearenv \
        --setenv HOME /home/u --setenv DISPLAY "$display" \
        --setenv SNAP "$snap_dir" --setenv SNAP_NAME "$name" \
        --setenv SNAP_LIBRARY_PATH "" \
        --setenv XDG_DATA_HOME /home/u/data --setenv XDG_CONFIG_HOME /home/u/config \
        --setenv PATH /usr/bin:/bin \
        /bin/sh /tmp/run/launch.sh >"$work/$label.log" 2>&1 &
    app_pid=$!
    sleep "$wait_s"
    if ! kill -0 "$app_pid" 2>/dev/null; then
        alive=no
    fi
    DISPLAY=$display import -window root "$work/$label.png"
    colors=$(identify -format '%k' "$work/$label.png")
    kill "$app_pid" 2>/dev/null || true
    wait "$app_pid" 2>/dev/null || true

    echo "--- $label: running after ${wait_s}s: $alive, screen colors: $colors"
    if [ "$alive" = no ]; then
        echo "::error::$label: md-viewer exited within ${wait_s}s"
        failed=1
    fi
    if grep -q 'panicked' "$work/$label.log"; then
        echo "::error::$label: md-viewer panicked"
        failed=1
    fi
    # An empty X root is one colour; the app's window with text is hundreds.
    if [ "$colors" -lt 32 ]; then
        echo "::error::$label: the screen is blank ($colors colours)"
        failed=1
    fi
    if [ -n "${SNAP_SMOKE_OUT:-}" ]; then
        mkdir -p "$SNAP_SMOKE_OUT"
        cp "$work/$label.log" "$work/$label.png" "$SNAP_SMOKE_OUT/"
    fi
    if [ "$failed" = 1 ]; then
        echo "--- $label log (tail):"
        tail -n 20 "$work/$label.log"
    fi
    return "$failed"
}

echo "[5/5] Running md-viewer (${wait_s}s each, with the host's fonts and with none)"
result=0
run_once with-fonts host || result=1
run_once no-fonts none || result=1

elapsed=$(( $(date +%s) - started ))
if [ "$result" = 0 ]; then
    echo "PASS: the snap starts and renders, with fonts and without (${elapsed}s)"
else
    echo "FAIL: the snap did not start cleanly in its base (${elapsed}s)"
fi
exit "$result"
