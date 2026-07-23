#!/usr/bin/env sh
# Sync (or verify) all vendored xahaud sources.
#
# Several parts of this workspace vendor small groups of files verbatim from
# xahaud's own source tree, rather than reimplementing or merely referencing
# them (see docs/DESIGN.md §6.5 and §4):
#   - guard-checker: the upstream guard checker itself
#     (crates/hooks-build/vendor/xahaud/VENDOR.md)
#   - hook-headers: the Hook API C headers, parity-tested against the Rust
#     translation in hooks-core
#     (crates/hooks-core/vendor/xahaud-hook/VENDOR.md)
#
# Each group has its own vendor directory and its own SHA256SUMS. These files
# are never hand-edited; this script is the only supported way to update
# them.
#
# Usage:
#   scripts/sync-vendor.sh           # update: download from upstream,
#                                    # overwrite vendored files, regenerate
#                                    # each group's SHA256SUMS (review with
#                                    # `git diff`)
#   scripts/sync-vendor.sh --check   # verify: fail (exit 1) if any group's
#                                    # vendored files differ from the
#                                    # upstream release branch or from its
#                                    # SHA256SUMS; writes nothing. Used by CI.
set -eu

REPO="Xahau/xahaud"
BRANCH="release"

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# Each group is "name:vendor_dir:upstream_path:files..." (space-separated
# file list as the trailing fields).
#
# name          human-readable label used in messages
# vendor_dir    directory (relative to repo root) holding the vendored files
#               and that group's own SHA256SUMS
# upstream_path path (relative to the branch root) the files live under

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

overall_status=0

# sync_group <name> <vendor_dir> <upstream_path> <files...>
sync_group() {
    name="$1"
    vendor_dir="${ROOT_DIR}/$2"
    upstream_path="$3"
    shift 3
    files="$*"

    base_url="https://raw.githubusercontent.com/${REPO}/refs/heads/${BRANCH}/${upstream_path}"
    sums_file="${vendor_dir}/SHA256SUMS"
    group_tmp="${TMP_DIR}/${name}"
    mkdir -p "${group_tmp}"

    echo "[${name}] fetching from ${REPO}@${BRANCH}/${upstream_path} ..."
    for f in ${files}; do
        if ! curl -sfL "${base_url}/${f}" -o "${group_tmp}/${f}"; then
            echo "error: failed to download ${base_url}/${f}" >&2
            exit 1
        fi
    done

    if [ "${MODE}" = "check" ]; then
        group_status=0

        # 1. Vendored files must match the upstream release branch.
        for f in ${files}; do
            if ! cmp -s "${group_tmp}/${f}" "${vendor_dir}/${f}"; then
                echo "DRIFT: [${name}] ${f} differs from upstream ${REPO}@${BRANCH}" >&2
                diff -u "${vendor_dir}/${f}" "${group_tmp}/${f}" | head -40 >&2 || true
                group_status=1
            fi
        done

        # 2. SHA256SUMS must match the vendored files on disk (hand-edit
        #    tripwire; the same file is asserted by the Rust test suite).
        for f in ${files}; do
            want="$(awk -v f="${f}" '$2 == f {print $1}' "${sums_file}")"
            got="$(sha256 "${vendor_dir}/${f}")"
            if [ "${want}" != "${got}" ]; then
                echo "DRIFT: [${name}] ${f} does not match SHA256SUMS (want ${want}, got ${got})" >&2
                group_status=1
            fi
        done

        if [ "${group_status}" -eq 0 ]; then
            echo "OK: [${name}] vendored files are byte-identical to ${REPO}@${BRANCH} and match SHA256SUMS"
        else
            overall_status=1
        fi
        return 0
    fi

    # update mode
    changed=0
    for f in ${files}; do
        if ! cmp -s "${group_tmp}/${f}" "${vendor_dir}/${f}"; then
            cp "${group_tmp}/${f}" "${vendor_dir}/${f}"
            echo "[${name}] updated: ${f}"
            changed=1
        else
            echo "[${name}] unchanged: ${f}"
        fi
    done

    # Regenerate this group's SHA256SUMS (two-space separator, sha256sum format).
    : > "${sums_file}"
    for f in ${files}; do
        printf '%s  %s\n' "$(sha256 "${vendor_dir}/${f}")" "${f}" >> "${sums_file}"
    done

    if [ "${changed}" -eq 1 ]; then
        echo "[${name}] vendored files updated. Review with:  git diff ${2}"
    else
        echo "[${name}] already in sync with ${REPO}@${BRANCH}"
    fi
}

sync_group "guard-checker" \
    "crates/hooks-build/vendor/xahaud" \
    "include/xrpl/hook" \
    Guard.h Enum.h hook_api.macro

sync_group "hook-headers" \
    "crates/hooks-core/vendor/xahaud-hook" \
    "hook" \
    error.h extern.h hookapi.h ls_flags.h macro.h sfcodes.h tts.h tx_flags.h

if [ "${MODE}" = "check" ]; then
    if [ "${overall_status}" -ne 0 ]; then
        echo "" >&2
        echo "Vendored sources have drifted. Run scripts/sync-vendor.sh to" >&2
        echo "re-sync from upstream, review the diff, and commit the result." >&2
    fi
    exit "${overall_status}"
fi

echo ""
echo "Done. Review any changes with:  git diff crates/hooks-build/vendor/ crates/hooks-core/vendor/"
echo "If the hook-headers group changed, regenerate hooks-core's translated sources:"
echo "  cargo xtask gen-core"
echo "Then run the test suite (vendored behavior/translations may have changed):"
echo "  cargo test --workspace"
