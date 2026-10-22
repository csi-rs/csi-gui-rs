#!/usr/bin/env bash
# Re-vendor the esp-csi-rs wire module into src/protocol/wire/.
#
# esp-csi-rs cannot build for a desktop target (its esp-hal / esp-radio
# dependencies are not optional), but its `wire` module has no `esp-*`
# imports by design, so host tools copy it verbatim. The only edit is
# turning the `synthetic` feature gate into `cfg(test)`: this crate uses
# the synthetic source for its own tests only.
#
# Usage: scripts/sync-wire.sh [path/to/esp-csi-rs]   (default ../esp-csi-rs)
#        scripts/sync-wire.sh --check [path]           (exit 1 on drift)
set -euo pipefail

check=0
if [[ "${1:-}" == "--check" ]]; then check=1; shift; fi
src="${1:-$(dirname "$0")/../../esp-csi-rs}"
here="$(cd "$(dirname "$0")/.." && pwd)"
dst="$here/src/protocol/wire"
version="$(grep -m1 '^version' "$src/Cargo.toml" | cut -d'"' -f2)"

render() {
    local f="$1"
    printf '// Vendored from esp-csi-rs %s, src/lib/wire/%s. Do not edit by hand:\n' "$version" "$f"
    printf '// run scripts/sync-wire.sh to update.\n'
    sed 's/#\[cfg(feature = "synthetic")\]/#[cfg(test)]/' "$src/src/lib/wire/$f"
}

status=0
for f in mod.rs layout.rs setup.rs synthetic.rs; do
    if (( check )); then
        if ! diff -q <(render "$f" | tail -n +3) <(tail -n +3 "$dst/$f") >/dev/null; then
            echo "drift: src/protocol/wire/$f differs from esp-csi-rs $version" >&2
            status=1
        fi
    else
        render "$f" > "$dst/$f"
    fi
done
(( check )) || echo "vendored esp-csi-rs $version wire module into $dst"
exit $status
