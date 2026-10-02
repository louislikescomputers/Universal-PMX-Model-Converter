#!/usr/bin/env bash
# compile-mac.sh - Build mmdconv for macOS 12+ (Intel x86_64 and Apple Silicon arm64)
#
# Usage:
#   ./compile-mac.sh                   # release build for the host arch
#   ./compile-mac.sh --debug           # debug build
#   ./compile-mac.sh --tests           # run the test suite first; abort on failure
#   ./compile-mac.sh --clippy          # run cargo clippy -D warnings first
#   ./compile-mac.sh --universal       # build both arm64 + x86_64 and lipo them
#                                      # into one fat binary (needs both rustup targets)
#
# Output binary is copied to ./dist/mmdconv together with a .sha256 checksum.
# The result is ad-hoc code-signed so it runs locally without Gatekeeper fuss.

set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")"

PROFILE="release"
PROFILE_FLAG="--release"
RUN_TESTS=0
RUN_CLIPPY=0
UNIVERSAL=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --debug)     PROFILE="debug"; PROFILE_FLAG="" ;;
        --tests)     RUN_TESTS=1 ;;
        --clippy)    RUN_CLIPPY=1 ;;
        --universal) UNIVERSAL=1 ;;
        -h|--help)   grep '^#' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        *) echo "ERROR: unknown option: $1 (see --help)" >&2; exit 2 ;;
    esac
    shift
done

if ! command -v cargo >/dev/null 2>&1; then
    echo "ERROR: 'cargo' was not found on PATH." >&2
    echo "Install Rust from https://rustup.rs" >&2
    exit 1
fi

HOST_ARCH="$(uname -m)"   # arm64 or x86_64
echo "== mmdconv build (macOS) =="
echo "   profile  : ${PROFILE}"
echo "   host arch: ${HOST_ARCH}"
echo "   toolchain: $(cargo --version)"

build_one() {
    local triple="$1" out="$2"
    if ! rustc --print target-list | grep -qx "$triple"; then
        echo "ERROR: unknown target triple: $triple" >&2
        exit 1
    fi
    rustup target add "$triple" >/dev/null 2>&1 || true
    echo
    echo "-- building (${triple}) --"
    # shellcheck disable=SC2086
    cargo build ${PROFILE_FLAG:+$PROFILE_FLAG} --bin mmdconv --target "$triple"
    local bin="target/${triple}/${PROFILE}/mmdconv"
    if [[ ! -f "$bin" ]]; then
        echo "ERROR: expected artifact not found: $bin" >&2
        exit 1
    fi
    cp -f "$bin" "$out"
}

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

mkdir -p dist

if [[ "$UNIVERSAL" == 1 ]]; then
    # Fat binary covering both Apple Silicon and Intel.
    case "$HOST_ARCH" in
        arm64)  OTHER=x86_64;  MAIN=arm64 ;;
        x86_64) OTHER=arm64;   MAIN=x86_64 ;;
        *) echo "ERROR: unexpected host arch: $HOST_ARCH" >&2; exit 1 ;;
    esac
    build_one "${MAIN}-apple-darwin"    "dist/.mmdconv-${MAIN}"
    build_one "${OTHER}-apple-darwin"   "dist/.mmdconv-${OTHER}"
    lipo -create -output dist/mmdconv "dist/.mmdconv-${MAIN}" "dist/.mmdconv-${OTHER}"
    rm -f "dist/.mmdconv-${MAIN}" "dist/.mmdconv-${OTHER}"
    echo "   created universal binary (arm64 + x86_64)"
else
    build_one "${HOST_ARCH}-apple-darwin" "dist/mmdconv"
fi

chmod +x dist/mmdconv

# Ad-hoc signature: required on Apple Silicon for the binary to launch at all,
# harmless on Intel. Uses no identity, so it only satisfies local execution.
codesign --sign - --force --timestamp=none dist/mmdconv 2>/dev/null || \
    echo "WARNING: ad-hoc codesign failed; the binary may need 'xattr -d com.apple.quarantine'."

if command -v shasum >/dev/null 2>&1; then
    ( cd dist && shasum -a 256 mmdconv > mmdconv.sha256 )
elif command -v sha256sum >/dev/null 2>&1; then
    ( cd dist && sha256sum mmdconv > mmdconv.sha256 )
fi

echo
echo "BUILD OK"
echo "  binary : $(pwd)/dist/mmdconv"
[[ -f dist/mmdconv.sha256 ]] && echo "  sha256 : $(awk '{print $1}' dist/mmdconv.sha256)"
echo "  size   : $(du -h dist/mmdconv | cut -f1)"
