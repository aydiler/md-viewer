#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

renderer_manifest="crates/egui_commonmark/Cargo.toml"
renderer_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$renderer_manifest" | head -n 1)
if [[ -z "$renderer_version" ]]; then
    echo "error: could not read renderer workspace version" >&2
    exit 1
fi

check_versions() {
    local manifest=$1
    local package=$2
    local found=0
    while IFS= read -r version; do
        found=1
        if [[ "$version" != "$renderer_version" ]]; then
            echo "error: $manifest pins $package $version; renderer workspace is $renderer_version" >&2
            exit 1
        fi
    done < <(sed -n "s/^${package} = {.*version = \"\([^\"]*\)\".*/\1/p" "$manifest")
    if [[ "$found" == 0 ]]; then
        echo "error: no versioned $package entry found in $manifest" >&2
        exit 1
    fi
}

check_versions Cargo.toml egui_commonmark_extended
check_versions Cargo.toml egui_commonmark_backend_extended
check_versions "$renderer_manifest" egui_commonmark_backend_extended
check_versions "$renderer_manifest" egui_commonmark_macros_extended

while read -r relative_path package_name; do
    manifest="$repo_root/$relative_path/Cargo.toml"
    if [[ ! -f "$manifest" ]]; then
        echo "error: patched crate manifest is missing: $manifest" >&2
        exit 1
    fi
    if ! grep -Fqx "name = \"$package_name\"" "$manifest"; then
        echo "error: $manifest does not declare package $package_name" >&2
        exit 1
    fi
done <<'EOF'
crates/egui_commonmark/egui_commonmark egui_commonmark_extended
crates/egui_commonmark/egui_commonmark_backend egui_commonmark_backend_extended
EOF

check_locked_metadata() {
    local messages
    if ! messages=$(CARGO_TERM_COLOR=never cargo metadata --locked --no-deps \
        --format-version 1 "$@" 2>&1 >/dev/null); then
        printf '%s\n' "$messages" >&2
        return 1
    fi
    if grep -Fq "was not used in the crate graph" <<<"$messages"; then
        printf 'error: Cargo reported an unused patch:\n%s\n' "$messages" >&2
        return 1
    fi
}

# `--locked` fails if either lockfile no longer represents its manifest. Cargo
# also reports patches that resolve to no package through metadata warnings.
check_locked_metadata
check_locked_metadata --manifest-path "$renderer_manifest"

# Source builds must resolve both renderer packages to their vendored paths,
# not same-version registry packages that make the local override ineffective.
dependency_tree=$(cargo tree --locked -p md-viewer -e normal)
while read -r relative_path package_name; do
    expected="${package_name} v${renderer_version} ($repo_root/$relative_path)"
    if ! grep -Fq "$expected" <<<"$dependency_tree"; then
        echo "error: root build does not resolve $package_name through its expected vendored path" >&2
        exit 1
    fi
done <<'EOF'
crates/egui_commonmark/egui_commonmark egui_commonmark_extended
crates/egui_commonmark/egui_commonmark_backend egui_commonmark_backend_extended
EOF

echo "workspace dependency contract is synchronized at renderer version $renderer_version"
