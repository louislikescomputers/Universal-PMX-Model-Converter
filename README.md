# mmdconv — Universal Rigged-Model → MikuMikuDance (PMX) Converter

`mmdconv` converts rigged 3D models (glTF/GLB, VRM, FBX, Collada, PMX/PMD,
and static meshes) into **PMX** files that load cleanly in MikuMikuDance
9.32+, PMXEditor, and three.js `MMDLoader` — with skeleton, skinning,
materials, textures, morphs, and optional physics. Zero manual steps for the
common case.

Written in Rust as a cargo workspace. Ships as a single self-contained binary
per platform: **no Python, no Blender, no .NET runtime required.**

> ⚠️ **Project status:** pre-release (`v0.1.0`). The PMX reader/writer/validator
> and the glTF/GLB/VRM importer are implemented; the FBX/DAE/OBJ importers,
> humanoid bone mapper, CLI subcommands, physics generation, and texture
> pipeline are in progress. See [docs/STATUS.md](docs/STATUS.md) for the exact
> per-feature matrix — do not assume a documented flag works until it is listed
> as ✅ there.

---

## Features

| | |
|---|---|
| **Formats in** | `.glb` / `.gltf` / `.vrm` (0.x & 1.0), planned: `.fbx` `.dae` `.pmx` `.pmd` `.obj` `.stl` `.ply` |
| **Format out** | PMX 2.0 (default) / PMX 2.1 (auto when needed), legacy PMD planned |
| **Skeleton** | Humanoid mapping (VRM roles → naming heuristics → geometry), standard MMD bones, leg/toe IK chains, twist/D-bones |
| **Skinning** | BDEF1/2/4, SDEF, QDEF; >4-influence truncation with report |
| **Materials** | PBR → MMD diffuse/spec/shininess/ambient, alpha modes, toon, sphere maps |
| **Textures** | PNG/JPG/TGA/BMP/WebP decode → re-encode beside output in `tex/`, ASCII-safe names |
| **Morphs** | Blendshapes / VRM expressions → まばたき・ウィンク・あいうえお… panel-mapped |
| **Physics** | VRM spring bones / dynamic chains → rigid bodies + joints (`--physics auto`) |
| **Robustness** | Magic-byte format detection, never panics on malformed input, deterministic byte-identical output, Unicode paths (Japanese filenames OK) |
| **Platforms** | Windows x64/arm64 · Linux x64/arm64 (glibc & musl) · macOS 12+ Intel & Apple Silicon |

## Installation

### Prebuilt binaries (recommended)

Grab the archive for your platform from the [GitHub Releases] page. Each
archive contains the binary and a `.sha256` checksum file.

| Platform | Archive |
|---|---|
| Windows 10/11 x64 | `mmdconv-windows-x64.zip` |
| Windows 11 arm64 | `mmdconv-windows-arm64.zip` |
| Linux x64 (glibc) | `mmdconv-linux-x64.tar.gz` |
| Linux arm64 / musl | `mmdconv-linux-arm64.tar.gz` / `-musl` |
| macOS universal (Intel + Apple Silicon) | `mmdconv-macos-universal.zip` |

Verify a checksum, e.g.:

```sh
sha256sum -c mmdconv.sha256            # Linux / macOS
certutil -hashfile dist\mmdconv.exe SHA256   # Windows
```

[GitHub Releases]: https://github.com/example/mmdconv/releases

### Build from source

Requires only a stable Rust toolchain (≥ 1.77): <https://rustup.rs>

Then use the per-platform compile script, which builds in release mode,
copies the binary to `dist/`, and writes a SHA-256 checksum:

| OS | Command |
|---|---|
| Windows (PowerShell) | `.\compile.ps1` |
| Windows (cmd) | `compile.bat` |
| macOS (Intel/ARM, `--universal` for fat binary) | `./compile-mac.sh` |
| Linux (x64/arm64, cross-targets supported) | `./compile-linux.sh` |

Or plain cargo:

```sh
cargo build --release            # binary at target/release/mmdconv
```

## Quick start

```sh
# Convert a GLB model; output goes beside the input as model.pmx (+ tex/)
mmdconv model.glb

# Explicit output path, A-pose normalization off, physics on
mmdconv character.vrm -o my_char.pmx --pose keep --physics auto

# Validate a produced file
mmdconv validate my_char.pmx

# Inspect any supported input: bone tree, mesh/material/morph stats
mmdconv inspect model.glb

# Batch-convert a folder, 4 parallel jobs, JSON report
mmdconv --batch ./models --jobs 4 --report ./models/report.json
```

After conversion, copy `output.pmx` **and** its sibling `tex/` folder together
into `MikuMikuDance\Model\` (preserving the relative `tex/…` paths) and open
the PMX in MMD or PMXEditor.

## Usage

```text
mmdconv <input> [-o output.pmx] [OPTIONS]
mmdconv validate <file.pmx>
mmdconv inspect  <file>
```

### Conversion options

| Flag | Default | Meaning |
|---|---|---|
| `--scale <f>` / `--height <m>` | auto (≈19.75 units ≈ 1.58 m) | Set output scale directly, or normalize total height to *m* meters |
| `--pose a\|t\|keep` | `a` | Normalize rest pose; skin is rebound so weights stay correct |
| `--physics auto\|off` | `auto` | Generate rigid bodies/joints from spring-bone data |
| `--physics-strength <f>` | `1.0` | Scale mass/gravity of generated physics |
| `--edge on\|off` | `on` | Outline (toon edge) on materials |
| `--toon none\|1..10` | `2` | Shared toon ramp (`toon01.bmp`…) |
| `--texture-format png\|jpg\|tga\|bmp` | `png` | Re-encode textures to this format |
| `--texture-dir <name>` | `tex` | Folder name for extracted textures |
| `--bone-map <file.toml>` | — | User bone-name overrides (see [BONE_MAPPING.md]) |
| `--pmx-version 2.0\|2.1` | `2.0` | 2.1 auto-selected when features require it (QDEF/soft body) |
| `--encoding utf16\|utf8` | `utf16` | PMX text encoding |
| `--no-ik` | — | Omit leg/toe IK chains |
| `--no-helper-bones` | — | Omit twist (腕捩/手捩), 肩P, D-bones |
| `--keep-unknown-bones` | — | Keep unmapped bones instead of pruning leaf extras |
| `--batch <dir>` | — | Recursive conversion; skips already-done files |
| `--jobs <N>` | CPU count | Parallelism for `--batch` |
| `--report <file.json>` | — | Machine-readable conversion report |
| `--dry-run` | — | Analyze + print plan; write nothing |
| `--verbose` / `--quiet` | — | Logging level |

Progress bars appear only when stdout is a TTY; color respects `NO_COLOR`.
Exit codes: `0` success · `1` conversion error · `2` CLI/usage error ·
`3` validation failed · `4` input unsupported.

Full flag reference and JSON-report schema: [docs/CLI.md].

## Supported formats & feature matrix

See **[docs/STATUS.md](docs/STATUS.md)** — it lists every pipeline stage with
implemented / partial / planned status, matching the code in this repository.
Highlights:

| Input | Detection | Status |
|---|---|---|
| GLB / glTF 2.0 | magic bytes + extension | ✅ importer implemented |
| VRM 0.x / 1.0 | glTF + `VRM`/`vcc` extension | ✅ humanoid roles, meta, expressions, spring bones read |
| FBX (binary/ASCII) | `Kaydara FBX Binary` magic | 🚧 planned (M7) |
| Collada `.dae` | XML root `<COLLADA>` | 🚧 planned (M7) |
| PMX / PMD | `PMX ` / `Pmd \0` magic | ✅ PMX reader done; PMD planned |
| OBJ / STL / PLY | sniffed text/binary | 🚧 planned — will emit mesh-only PMX + warning |

## Loading the result in MMD

1. Copy `model.pmx` and its `tex/` folder into `MikuMikuDance\Model\`.
2. Launch `MikuMikuDance.exe` → *File → Load Model*.
3. If the model appears tiny/huge, check you didn't double-scale — mmdconv
   already normalizes to ~19.75 units tall; use `--height` to override.
4. Missing textures? The PMX stores **relative** paths (`tex/foo.png`); the
   `tex/` folder must sit next to the `.pmx`.

## Documentation

| Document | Contents |
|---|---|
| [docs/ARCHITECTURE.md] | Pipeline stages, intermediate representation (IR), crate layout |
| [docs/CLI.md] | Complete command reference, exit codes, JSON report schema |
| [docs/PMX_SPEC_NOTES.md] | Exactly how we write PMX 2.0/2.1 and where we deviate |
| [docs/COMPILING.md] | Toolchains, per-OS notes, CI artifacts |
| [docs/STATUS.md] | Honest milestone/feature status |
| [DECISIONS.md] | Architecture choices and defaults, with rationale |
| [BONE_MAPPING.md] | Mapping tables, heuristics, `--bone-map` TOML schema |
| [CHANGELOG.md] | Release history (semantic versioning) |
| [CONTRIBUTING.md] | Dev setup, tests, style gates |
| [SECURITY.md] | Threat model for untrusted input files, fuzzing |

## Testing & quality gates

```sh
cargo test --workspace          # unit + golden-file + round-trip tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
cargo audit                     # cargo-audit, run in CI nightly too
```

Acceptance criteria (from the project charter): round-trip equality through
our own PMX reader, independent cross-check via three.js `MMDLoader` in CI,
skinning bind-pose verification, identical output hashes across all six CI
targets (Windows, Linux x64/arm64, macOS Intel/ARM). See [CONTRIBUTING.md].

## Known limitations (current release)

- Only glTF/GLB/VRM import is wired up end-to-end; other formats error with a
  clear "unsupported" message rather than silently mis-converting.
- The CLI binary currently exposes the core pipeline only partially — flags
  documented above describe the target interface; consult [docs/STATUS.md].
- SDEF vertices are written with zero control-point params (lossy) pending
  implementation.
- No `.blend`/USD support even behind feature flags yet.

## License

MIT (see [LICENSE]). Model **content** licenses are independent: mmdconv
copies source license/author metadata (VRM meta, glTF asset extras) into the
PMX comment fields and prints a warning when the source restricts
redistribution. Always respect the original artist's terms.

## Acknowledgements

MikuMikuDance by Yu Higuchi (みくたん); the PMX community spec;
three.js `MMDLoader`; VRM Consortium specifications.

[docs/ARCHITECTURE.md]: docs/ARCHITECTURE.md
[docs/CLI.md]: docs/CLI.md
[docs/PMX_SPEC_NOTES.md]: docs/PMX_SPEC_NOTES.md
[docs/COMPILING.md]: docs/COMPILING.md
[docs/STATUS.md]: docs/STATUS.md
[DECISIONS.md]: DECISIONS.md
[BONE_MAPPING.md]: BONE_MAPPING.md
[CHANGELOG.md]: CHANGELOG.md
[CONTRIBUTING.md]: CONTRIBUTING.md
[SECURITY.md]: SECURITY.md
[LICENSE]: LICENSE
