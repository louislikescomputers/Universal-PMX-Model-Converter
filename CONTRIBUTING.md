# Contributing

Thanks for helping build mmdconv! This repo converts rigged 3D models to
MikuMikuDance PMX; correctness and reproducibility matter more than speed of
feature delivery.

## Dev setup

1. Install Rust via [rustup](https://rustup.rs) (stable ≥ 1.77).
2. Clone and build:
   ```sh
   git clone https://github.com/example/mmdconv
   cd mmdconv
   cargo build --release          # or ./compile-linux.sh / .\compile.ps1 / ./compile-mac.sh
   ```
3. Optional tools used by CI: `cargo-clippy`, `cargo-fmt` (both from rustup),
   `cargo-audit`, Node.js ≥ 18 (for the three.js MMDLoader cross-check script).

## Build, test, lint

```sh
cargo test --workspace                                  # all unit + golden tests
cargo clippy --workspace --all-targets -- -D warnings   # must be clean
cargo fmt --all --check                                 # must be clean
cargo audit                                             # must be clean (RUSTSEC triage below)
```

Every PR runs the full matrix: windows-latest, ubuntu-latest, macos-latest
(ARM), macos-13 (Intel). **Golden-file outputs must hash identically across
all platforms** — if your change alters output bytes, regenerate goldens in
the same commit and say why in the PR description.

## Project layout & where things go

| Area | Location | Milestone tracker |
|---|---|---|
| IR types | `crates/mmdconv-core/src/ir.rs` | — |
| Importers | `src/importers/<format>.rs` (+ magic-byte dispatch in `mod.rs`) | docs/STATUS.md |
| Pipeline stages | `normalize.rs`, `skeleton/`, `mesh.rs`, `materials.rs`, `morphs.rs`, `physics.rs` | docs/STATUS.md |
| PMX codec | `src/pmx/{model,reader,writer,validate}.rs` | M1 |
| CLI | `crates/mmdconv-cli/src/main.rs` | M9 |
| Test fixtures | `tests/fixtures/` (tiny CC0 only!) | — |

See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) for the data flow and
[DECISIONS.md](DECISIONS.md) before proposing architectural changes — open an
issue first if you plan to supersede a decision.

## Testing rules

* **Every stage gets unit tests** in-module (`#[cfg(test)]`) plus integration
  coverage under `tests/`.
* New importer ⇒ golden test: import → assert IR structure → convert →
  validate → round-trip read → structural compare.
* Parsers must survive garbage: add random/truncated-byte cases. **No code
  path may panic on malformed input** — use `Result`, avoid `unwrap()` outside
  tests (clippy config enforces this).
* Lossy conversions must emit warnings and report counters; a silent
  degradation is a bug even if output "looks fine".
* Fuzz targets live in `fuzz/` (cargo-fuzz); run locally with
  `cargo +nightly fuzz run pmx_reader` when touching readers/writers.

## Code style

* `cargo fmt` defaults; clippy `-D warnings` is the law.
* Determinism discipline: never iterate HashMaps whose order affects output —
  use `IndexMap` or sort explicitly. No timestamps/random values in emitted
  files.
* Public API errors are `MmdconvError` variants with human fix-hints; keep
  messages actionable ("hint: …").
* Comments explain *why*; spec citations (PMX format docs, VRM spec sections)
  belong next to the code that implements them.

## Commit & PR conventions

* Conventional commits: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`,
  `chore:`; scope optional (`pmx:`, `gltf:`, `cli:`).
* Small commits, one logical change each. Keep milestone work in branches like
  `m4-skeleton-builder`.
* PR template checklist: tests added ✅ · STATUS.md updated ✅ · CHANGELOG.md
  entry ✅ · no new clippy/fmt/audit findings ✅ · cross-platform hashes green ✅.

## Updating documentation

Docs are part of the deliverable, not an afterthought:

* New user-visible flag ⇒ update README table **and** docs/CLI.md.
* Feature status change ⇒ update docs/STATUS.md in the same PR (policy D-27).
* Mapping-table change ⇒ BONE_MAPPING.md + its unit-test fixture together.

## Sample models & licensing

Fixtures must be tiny and **CC0/public domain**, generated in code where
possible. Never commit ripped MMD models, VRM marketplace assets, or anything
whose license forbids redistribution. If you need a real-world rig for
testing, put it in `tests/manual/` (gitignored) and reference how to obtain it.

## Reporting issues

Include: input file (or minimal repro), exact command + flags, expected vs.
actual, `mmdconv inspect` output, OS/arch, binary version/hash. For crashes
on malformed files, attach the offending bytes — those are fuzz-seed material.

## License note

By contributing you confirm your contributions are licensed under the
project's MIT license.
