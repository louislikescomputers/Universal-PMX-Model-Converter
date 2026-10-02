# Changelog

All notable changes to **mmdconv** follow [Semantic Versioning](https://semver.org/)
and this file follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added
- Per-platform build scripts: `compile.ps1`, `compile.bat`,
  `compile-linux.sh`, `compile-mac.sh` (release build → `dist/` + SHA-256;
  macOS script adds `--universal` lipo builds and ad-hoc codesigning).
- Full documentation set: README, docs/ARCHITECTURE.md, docs/CLI.md,
  docs/PMX_SPEC_NOTES.md, docs/COMPILING.md, docs/STATUS.md, DECISIONS.md,
  BONE_MAPPING.md, CONTRIBUTING.md, SECURITY.md.

### Fixed
- (none yet)

## [0.1.0] - 2026-10-02

First development snapshot. Not feature-complete; see docs/STATUS.md for the
capability ledger.

### Added
- **Cargo workspace**: `mmdconv-core` library + `mmdconv-cli` binary stub.
- **PMX data model** (`pmx/model.rs`): full PMX 2.0/2.1 structure — dynamic
  index sizes (U8/U16/U32), UTF-16LE/UTF-8 text, all vertex deform types
  (BDEF1/2/4, SDEF, QDEF), material flags/toon/sphere modes, complete bone
  flag semantics (IK, append inheritance, fixed axis, local axes, external
  parent), all morph kinds (vertex/UV/bone/impulse/material/flip/group),
  display frames, rigid bodies, joints.
- **PMX writer** (`pmx/writer.rs`): byte-deterministic, spec-exact header &
  globals, automatic smallest-legal index sizes, mandatory validation gate
  before emission (face bounds, weight sums, bone-parent cycles, IK link
  integrity, NaN rejection).
- **PMX reader** (`pmx/reader.rs`): bounds-checked parser for 2.0/2.1 in both
  encodings; panic-free on malformed input (random-byte fuzz unit test);
  deferred skin-bone reference resolution; material-morph format variants;
  soft-body section preserved raw for lossless re-export.
- **Standalone validator** (`pmx/validate.rs`): counts vs. index sizes,
  cycle detection (tortoise-and-hare), IK loop/angle/link checks, morph
  offset ranges, rigid/joint cross-references, texture path existence &
  relativity.
- **IR** (`ir.rs`): format-agnostic model — nodes/skeleton with global
  transforms, meshes with unlimited skin influences, PBR materials + texture
  slots, morph targets, VRM spring-bone/collider descriptors, bilingual text,
  coordinate-system descriptor, pose kind; full VRM `SemanticRole` set.
- **glTF importer** (`importers/gltf.rs`): GLB container parsing
  (magic/version/chunk validation), accessors (all component types, strides,
  normalized attributes), node TRS → bones, skins/joints/inverse bind
  matrices, primitive→triangle material groups, TEXCOORD_0/1, morph-target
  offsets, VRM extension handling (humanoid roles, meta/license, expressions,
  spring bones), T/A pose heuristic, KHR_materials_* warnings.
- **Importer helpers** (`importers/common.rs`): n-gon triangulation, smooth
  normals, top-4 weight truncation with renormalization + reporting,
  epsilon vertex dedup with weight-signature hashing, strict depth-limited
  JSON parser (surrogate-pair aware).
- Error type with human fix-hints (`error.rs`).

### Known limitations
- CLI binary is a stub; conversion flow not wired end-to-end yet.
- FBX/Collada/OBJ/PMD importers, MMD skeleton builder, pose rebinding,
  material/texture emission, morph mapping, physics generation pending.
- SDEF control-point params written as zeros (lossy placeholder).

[Unreleased]: https://github.com/example/mmdconv/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/example/mmdconv/releases/tag/v0.1.0
