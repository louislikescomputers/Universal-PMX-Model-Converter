# Compiling mmdconv

You need a stable Rust toolchain (≥ 1.77): install via
[rustup](https://rustup.rs). Nothing else — no Python, no Blender, no .NET.

The repository ships one build script per platform. All four do the same
thing: preflight the toolchain, build `--release` (unless told otherwise),
copy the resulting `mmdconv` binary into `dist/`, and write a SHA-256
checksum file next to it. They exit non-zero with a clear message if any step
fails.

## Windows

### compile.ps1 (PowerShell 5.1+ / PowerShell 7)

```powershell
.\compile.ps1                 # release build -> dist\mmdconv.exe (+ .sha256)
.\compile.ps1 -BuildDebug     # debug build (note: NOT -Debug; that name collides with PowerShell's common parameter)
.\compile.ps1 -Tests          # run cargo test first, abort on failure
.\compile.ps1 -Clippy         # gate on clippy -D warnings
.\compile.ps1 -Triple aarch64-pc-windows-msvc   # cross-compile (needs rustup target)
```

If script execution is blocked:
`Set-ExecutionPolicy -Scope Process Bypass` or run
`powershell -ExecutionPolicy Bypass -File .\compile.ps1`.

### compile.bat (cmd.exe)

```bat
compile.bat            :: release
compile.bat debug      :: debug build
compile.bat tests      :: run the test suite first
```

Uses `where cargo` to preflight and `certutil -hashfile` for the checksum
(no external tools). Checksum line format differs slightly from the shell
scripts because `certutil` prints its own style; the `.sha256` file is
normalized to `<hash>  mmdconv.exe` anyway.

Build prerequisites: MSVC Build Tools (or the GNU toolchain you chose with
rustup). For arm64 Windows installs, add the target once:
`rustup target add aarch64-pc-windows-msvc`.

## Linux — compile-linux.sh

```sh
./compile-linux.sh                       # host release build
./compile-linux.sh --debug
./compile-linux.sh --tests               # test suite must pass first
./compile-linux.sh --clippy              # -D warnings gate
./compile-linux.sh --target x86_64-unknown-linux-musl
./compile-linux.sh --target aarch64-unknown-linux-gnu
```

* The `--target` triple is validated against `rustc --print target-list` and
  the rustup target is installed automatically if missing.
* **musl builds** require `musl-gcc` (Debian/Ubuntu: `apt install musl-tools`;
  Alpine works out of the box). This is how we produce fully static glibc-
  free binaries for older servers.
* Cross-compiling arm64 from x64 needs `gcc-aarch64-linux-gnu` (or use QEMU
  CI runners).
* Output: `dist/mmdconv`, `dist/mmdconv.sha256`. Binary is chmod +x'd.

## macOS — compile-mac.sh

```sh
./compile-mac.sh                # host arch (arm64 or x86_64)
./compile-mac.sh --universal    # both arches, lipo'd into one fat binary
./compile-mac.sh --debug
./compile-mac.sh --tests
./compile-mac.sh --clippy
```

Requirements: Xcode Command Line Tools (`xcode-select --install`).

What the script does that plain `cargo build` does not:

1. Builds with an explicit `<arch>-apple-darwin` target so cross-arch builds
   from either machine type work.
2. With `--universal`, builds `aarch64-apple-darwin` **and**
   `x86_64-apple-darwin`, then `lipo -create` merges them.
3. **Ad-hoc codesigns** the result (`codesign --force --sign -`). Apple
   Silicon refuses to run unsigned arm64 binaries at all; this makes the
   local build immediately runnable. Distributable builds should be signed
   and notarized with a real Developer ID (CI artifacts are ad-hoc signed
   only — see below).
4. Writes `dist/mmdconv` + `dist/mmdconv.sha256`.

Gatekeeper note: users downloading a CI-built binary may need right-click →
Open (or `xattr -d com.apple.quarantine mmdconv`) until formal notarization
is set up.

## Plain cargo (any platform)

```sh
cargo build --release
# binary: target/release/mmdconv(.exe)
```

Workspace layout: `crates/mmdconv-core` (library) and `crates/mmdconv-cli`
(the `mmdconv` binary). Release profile enables thin LTO.

## CI & release artifacts

GitHub Actions (`.github/workflows/`) builds on `windows-latest`,
`ubuntu-latest`, `macos-latest` (ARM) and `macos-13` (Intel), runs the full
test suite on every push, and on tags produces zipped/tarred binaries with
`.sha256` checksums. Golden-file outputs must hash identically across all
runners — determinism failures block the release.

## Troubleshooting

| Symptom | Fix |
|---|---|
| `cargo not found` | Install rustup; restart the shell so PATH updates apply. |
| Link errors on Linux musl | `apt install musl-tools` (or build on Alpine). |
| "killed" immediately on macOS ARM | Re-run `compile-mac.sh` — the ad-hoc signature was missing (git checkouts don't preserve signatures). |
| PowerShell blocks `compile.ps1` | `Set-ExecutionPolicy -Scope Process Bypass` |
| Antivirus flags `mmdconv.exe` | Unsigned binaries sometimes trip SmartScreen; verify the published SHA-256 and report false positives. |
| Non-ASCII paths break in cmd | Use `chcp 65001` or the PowerShell script (handles Unicode paths natively). |
