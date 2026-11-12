#!/usr/bin/env bash
# Build and publish the snap on a GitHub runner, retrying every Snap Store call.
#
#   snap-ci.sh setup              install LXD + snapcraft, prepare LXD networking
#   snap-ci.sh build              snapcraft in LXD; prints the .snap path to $GITHUB_OUTPUT (snap=...)
#   snap-ci.sh publish <file>     upload + release to stable, then upload-metadata
#
# Replaces snapcore/action-build + action-publish, which call the store once per
# step with no retry. On 2026-11-12 the store dropped roughly one request in
# twelve, and the v0.3.2 snap job failed six times in a row at `snap install lxd`
# before anything was built (docs/devlog/078-snap-store-retry.md). The steps
# below mirror action-build's (src/tools.ts, src/build.ts) one for one.
set -euo pipefail

# retry <attempts> <cmd...>: back off 15 s, 30 s, 45 s, ... between attempts.
retry() {
    local attempts=$1; shift
    local i
    for ((i = 1; i <= attempts; i++)); do
        if "$@"; then
            return 0
        fi
        if ((i < attempts)); then
            echo "::warning::attempt $i/$attempts failed: $*; retrying in $((i * 15)) s"
            sleep $((i * 15))
        fi
    done
    echo "::error::failed after $attempts attempts: $*"
    return 1
}

setup() {
    # GitHub's runner image has odd ownership on / that trips up snap-confine.
    if [ "$(stat -c %u:%g /)" != "0:0" ]; then
        sudo chown root:root /
    fi
    if [ -x /usr/bin/lxd ]; then
        sudo apt-get remove -qy lxd lxd-client
    fi
    sudo groupadd --force --system lxd
    sudo usermod --append --groups lxd "$USER"

    # `snap install` of an already-installed snap exits 0 without asking the
    # store, so a retry after a partial success is safe.
    retry 6 sudo snap install lxd
    sudo lxd init --auto
    # Docker on the runner sets FORWARD to DROP, which cuts LXD containers off.
    sudo iptables -P FORWARD ACCEPT

    retry 6 sudo snap install snapcraft --classic --channel stable
}

build() {
    local image_info
    image_info=$(printf '{"build_url": "https://github.com/%s/actions/runs/%s"}' \
        "$GITHUB_REPOSITORY" "$GITHUB_RUN_ID")
    # snapcraft pulls the base and build snaps from the store inside the
    # container, so the whole build is retried too. A retry reuses the
    # instance, so only the failed part is repeated.
    retry 3 sudo -u "$USER" -E env \
        SNAPCRAFT_BUILD_ENVIRONMENT=lxd \
        SNAPCRAFT_BUILD_INFO=1 \
        SNAPCRAFT_IMAGE_INFO="$image_info" \
        snapcraft

    local snaps=(./*.snap)
    if [ "${#snaps[@]}" -ne 1 ] || [ ! -f "${snaps[0]}" ]; then
        echo "::error::expected exactly one .snap, found: ${snaps[*]}"
        return 1
    fi
    echo "built ${snaps[0]}"
    echo "snap=${snaps[0]}" >> "$GITHUB_OUTPUT"
}

publish() {
    local snap=${1:?usage: snap-ci.sh publish <file.snap>}
    : "${SNAPCRAFT_STORE_CREDENTIALS:?SNAPCRAFT_STORE_CREDENTIALS is not set}"
    retry 4 snapcraft upload --release=stable "$snap"
    # The listing description lives in the snap's embedded snapcraft.yaml;
    # --force treats the repository as the source of truth over web-UI edits.
    retry 4 snapcraft upload-metadata "$snap" --force
}

case "${1:-}" in
    setup) setup ;;
    build) build ;;
    publish) shift; publish "$@" ;;
    *) echo "usage: $0 setup|build|publish <file.snap>" >&2; exit 2 ;;
esac
