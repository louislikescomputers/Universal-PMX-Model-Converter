#!/usr/bin/env bash
# compile-linux.sh - Build mmdconv for Linux (x64 / arm64, glibc or musl)
#
# Usage:
#   ./compile-linux.sh                 # release build for the host triple
#   ./compile-linux.sh --debug         # debug build
#   ./compile-linux.sh --tests         # run the test suite first; abort on failure
#   ./compile-linux.sh --clippy        # run cargo clippy -D warnings first
#   ./compile-linux.sh --target x86_64-unknown-linux-musl   # cross/static build
#
# Output binary is copied to ./dist/mmdconv together with a .sha256 checksum.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

PROFILE="release"
PROFILE_FLAG="--release"
RUN_TESTS=0
RUN_CLIPPY=0
TRIPLE=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug)   PROFILE="debug"; PROFILE_FLAG="" ;;
        --tests)   RUN_TESTS=1 ;;
        --clippy)  RUN_CLIPPY=1 ;;
        --target)  TRIPLE="${2:?--target requires a value}"; shift ;;
        -h|--help) grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "ERROR: unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: 'cargo' was not found on PATH." >&2
    echo "Install Rust from https://rustup.rs (curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh)" >&2
    exit 1
fi

echo "== mmdconv build (Linux) =="
echo "   profile  : ${PROFILE}"
echo "   toolchain: $(cargo --version)"
[[ -n "$TRIPLE" ]] && echo "   target   : ${TRIPLE}"

TARGET_ARGS=()
if [[ -n "$TRIPLE" ]]; then
    TARGET_ARGS=(--target "$TRIPLE")
    if ! rustc --print target-list | grep -qx "$TRIPLE"; then
        echo "ERROR: unknown target triple: $TRIPLE" >&2
        exit 1
    fi
    # Make sure the std/core for the target is installed (no-op when it is).
    rustup target add "$TRIPLE" >/dev/null 2>&1 || true
fi

if [[ "$RUN_CLIPPY" == 1 ]]; then
    echo
    echo "-- clippy (-D warnings) --"
    cargo clippy --workspace --all-targets -- -D warnings
fi

if [[ "$RUN_TESTS" == 1 ]]; then
    echo
    echo "-- test suite --"
    # shellcheck disable=SC2086
    cargo test --workspace ${PROFILE_FLAG:+$PROFILE_FLAG}
fi

echo
echo "-- building mmdconv --"
# shellcheck disable=SC2086
cargo build ${PROFILE_FLAG:+$PROFILE_FLAG} --bin mmdconv "${TARGET_ARGS[@]}"

if [[ -n "$TRIPLE" ]]; then
    SRC="target/${TRIPLE}/${PROFILE}/mmdconv"
else
    SRC="target/${PROFILE}/mmdconv"
fi

if [[ ! -f "$SRC" ]]; then
    echo "ERROR: expected artifact not found: $SRC" >&2
    exit 1
fi

mkdir -p dist
cp -f "$SRC" dist/mmdconv
chmod +x dist/mmdconv

if command -v sha256sum >/dev/null 2>&1; then
    ( cd dist && sha256sum mmdconv > mmdconv.sha256 )
elif command -v shasum >/dev/null 2>&1; then
    ( cd dist && shasum -a 256 mmdconv > mmdconv.sha256 )
fi

echo
echo "BUILD OK"
echo "  binary : $(pwd)/dist/mmdconv"
[[ -f dist/mmdconv.sha256 ]] && echo "  sha256 : $(awk '{print $1}' dist/mmdconv.sha256)"
echo "  size   : $(du -h dist/mmdconv | cut -f1)"
