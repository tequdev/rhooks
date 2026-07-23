#!/usr/bin/env sh
# Sync (or verify) the vendored xahaud guard-checker sources.
#
# The authoritative guard-checker verdict comes from xahaud's own source,
# vendored byte-identical into crates/hooks-build/vendor/xahaud/ (see
# docs/DESIGN.md §6.5 and vendor/xahaud/VENDOR.md). These files are never
# hand-edited; this script is the only supported way to update them.
#
# Usage:
#   scripts/sync-vendor.sh           # update: download from upstream,
#                                    # overwrite vendored files, regenerate
#                                    # SHA256SUMS (review with `git diff`)
#   scripts/sync-vendor.sh --check   # verify: fail (exit 1) if the vendored
#                                    # files differ from the upstream release
#                                    # branch or from SHA256SUMS; writes
#                                    # nothing. Used by CI.
set -eu

REPO="Xahau/xahaud"
BRANCH="release"
BASE_URL="https://raw.githubusercontent.com/${REPO}/refs/heads/${BRANCH}/include/xrpl/hook"
FILES="Guard.h Enum.h hook_api.macro"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
VENDOR_DIR="${ROOT_DIR}/crates/hooks-build/vendor/xahaud"
SUMS_FILE="${VENDOR_DIR}/SHA256SUMS"

MODE="update"
if [ "${1:-}" = "--check" ]; then
    MODE="check"
elif [ -n "${1:-}" ]; then
    echo "usage: $0 [--check]" >&2
    exit 2
fi

# sha256 <file> — portable across macOS (shasum) and Linux (sha256sum).
sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        shasum -a 256 "$1" | awk '{print $1}'
    fi
}

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT INT TERM

echo "fetching from ${REPO}@${BRANCH} ..."
for f in ${FILES}; do
    if ! curl -sfL "${BASE_URL}/${f}" -o "${TMP_DIR}/${f}"; then
        echo "error: failed to download ${BASE_URL}/${f}" >&2
        exit 1
    fi
done

if [ "${MODE}" = "check" ]; then
    status=0

    # 1. Vendored files must match the upstream release branch.
    for f in ${FILES}; do
        if ! cmp -s "${TMP_DIR}/${f}" "${VENDOR_DIR}/${f}"; then
            echo "DRIFT: ${f} differs from upstream ${REPO}@${BRANCH}" >&2
            diff -u "${VENDOR_DIR}/${f}" "${TMP_DIR}/${f}" | head -40 >&2 || true
            status=1
        fi
    done

    # 2. SHA256SUMS must match the vendored files on disk (hand-edit tripwire;
    #    the same file is asserted by the Rust test suite).
    for f in ${FILES}; do
        want="$(awk -v f="${f}" '$2 == f {print $1}' "${SUMS_FILE}")"
        got="$(sha256 "${VENDOR_DIR}/${f}")"
        if [ "${want}" != "${got}" ]; then
            echo "DRIFT: ${f} does not match SHA256SUMS (want ${want}, got ${got})" >&2
            status=1
        fi
    done

    if [ "${status}" -eq 0 ]; then
        echo "OK: vendored files are byte-identical to ${REPO}@${BRANCH} and match SHA256SUMS"
    else
        echo "" >&2
        echo "Vendored guard-checker sources have drifted. Run scripts/sync-vendor.sh" >&2
        echo "to re-sync from upstream, review the diff, and commit the result." >&2
    fi
    exit "${status}"
fi

# update mode
changed=0
for f in ${FILES}; do
    if ! cmp -s "${TMP_DIR}/${f}" "${VENDOR_DIR}/${f}"; then
        cp "${TMP_DIR}/${f}" "${VENDOR_DIR}/${f}"
        echo "updated: ${f}"
        changed=1
    else
        echo "unchanged: ${f}"
    fi
done

# Regenerate SHA256SUMS (two-space separator, sha256sum format).
: > "${SUMS_FILE}"
for f in ${FILES}; do
    printf '%s  %s\n' "$(sha256 "${VENDOR_DIR}/${f}")" "${f}" >> "${SUMS_FILE}"
done

if [ "${changed}" -eq 1 ]; then
    echo ""
    echo "Vendored files updated. Review with:  git diff crates/hooks-build/vendor/"
    echo "Then run the test suite (the native checker behavior may have changed):"
    echo "  cargo test -p hooks-build"
else
    echo "already in sync with ${REPO}@${BRANCH}"
fi
