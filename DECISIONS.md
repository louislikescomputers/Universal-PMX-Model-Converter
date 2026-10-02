# DECISIONS.md — architecture choices & defaults

Each decision has an ID for cross-references. "Context → Decision → Why →
Alternatives rejected" format. Update rather than delete; superseded entries
get marked **[superseded by D-nn]**.

## Toolchain & project shape

**D-01 Rust, stable, cargo workspace (core lib + CLI bin).**
Context: single self-contained binaries per OS, no runtime deps.
Why: memory safety on hostile input files without GC/runtime payload; easy
cross-compilation; strong byte-manipulation ergonomics for the PMX codec.
Rejected: Go (weaker zero-cost abstraction fit for SIMD-ish geometry work,
larger static binaries), C++ (unsafe parsing risk), Python+PyInstaller
(violates "no runtime installs").

**D-02 Own hand-written PMX reader/writer instead of wrapping a library.**
Why: byte-exact determinism, spec-level control of index sizes/encodings,
and the reader doubles as the validator's parser. Rejected: binding to
C++ PMX libs (breaks single-binary goal).

**D-03 Crate choices:** `gltf`-style custom lightweight JSON+GLB parser
(shipped in-tree) instead of the `gltf` crate — we need morph/skin/VRM
details the crate flattens, and fewer transitive deps. `image` (subset of
codecs), `glam` (SIMD-friendly math, serde feature for configs), `clap` v4
derive, `serde`+`toml` config, `rayon` parallelism, `anyhow` at CLI edge /
`thiserror` in library, `tracing` logging. `ufbx` deferred: FBX will use the
`ufbx` C library via bindgen **only if** a pure-Rust path proves too slow —
decision postponed to M7 (documented then).

**D-04 Magic-byte-first format detection**, extension as tie-breaker only.
Context: `.vrm` files are glTF; renamed `.glb`s are common in the wild.

## Geometry & scale

**D-05 Default height normalization to 1.58 m ≈ 19.75 MMD units.**
Why: matches typical MMD character size band (18–20 units); overrides via
`--scale`/`--height`.

**D-06 RH→LH conversion mirrors the X axis and reverses triangle winding.**
Why: mirroring X keeps Y-up and handedness flip minimal; consistent negation
of normals' X and bitangent sign keeps lighting correct. Rejected: swapping
index order only (leaves normals wrong).

**D-07 IR stores source space + `CoordSystem`; normalization is one pass.**
Why: importers stay dumb/testable; every downstream stage sees one frame.

**D-08 Default pose target A-pose (`--pose a`) with skin rebinding.**
Why: MMD standards expect ~45° arms; rebinding (transform vertices by bind
matrices, rebuild inverse bind poses) keeps deformation correct. Default can
be flipped per-model with `--pose keep`.

## Skeleton

**D-09 Mapping priority: VRM humanoid → known naming conventions → geometry
heuristics.** Full tables in BONE_MAPPING.md. Non-humanoid fallback keeps
original hierarchy under 全ての親/センター with **no IK** and a loud warning.

**D-10 Standard bone names written in Japanese (UTF-16), EN aliases kept.**
Why: MMD features (morph panels, physics bone-follow, camera lookAt) key off
exact JP names.

**D-11 Knee IK limits: X-axis only, min angle > 0, loop 7, angle limit π.**
Why: prevents hyperextension/backward bending — the classic converter bug.

## Materials

**D-12 PBR→MMD approximation formulas** (shininess from roughness
`pow(2, log2…)` clamp [2, 64]; specular tint desaturated by metallic;
ambient = diffuse×0.1 + emissive). Defaults chosen to look right with MMD's
toon shading; exact constants live in `materials.rs` with golden-image tests
once rendering checks exist.

**D-13 Textures always re-encoded beside output into `tex/` with ASCII-safe
deduplicated names; PMX stores forward-slash relative paths.**
Why: kills the #1 MMD support issue (absolute/backslash/Japanese texture
paths). Alpha preserved unless target format can't carry it (then warn).

**D-14 Toon default = shared ramp `toon02.bmp`; sphere maps off.**
Conservative, closest to stock MMD look. Config-overridable.

## Skinning

**D-15 Keep top-4 influences, renormalize, pick smallest BDEF type, report
truncation count.** Silent degradation forbidden.

**D-16 Unskinned meshes attached to nodes → BDEF1 rigid bind to that node's
bone.** Matches how MMD users expect props/accessories to behave.

## Morphs

**D-17 Map VRM/ARKit/VRoid names to standard MMD morphs (まばたき, ウィンク,
あいうえお, にこり, 困る, 怒り, 笑い…) with panel assignment; unmapped keep
original name under "Other".** Split/combine rules (VRM blink L/R → まばたき
+ individualウィンク) table in BONE_MAPPING.md.

## Physics

**D-18 `--physics auto` default-on but no-op-with-warning when source lacks
spring data. Conservative mass/damping defaults, group masks avoid body
self-collision, capsule colliders for torso/legs.** Stability over realism;
`--physics-strength` scales liveliness.

## Output & robustness

**D-19 PMX 2.0 default; auto-bump to 2.1 only when features require it
(QDEF/soft body); explicit `--pmx-version` cannot downgrade past a hard
requirement (errors instead).**

**D-20 UTF-16LE text encoding default.** Safest for JP names across MMD,
PMXEditor, three.js.

**D-21 Validation is a mandatory writer gate + standalone command.** A file
that fails our validator cannot be produced by us, period.

**D-22 Never panic on malformed input; parsers bounds-check every read.**
Fuzzed via random-byte streams now; cargo-fuzz harness scheduled M10.

**D-23 Byte-deterministic output** (stable orders via IndexMap, fixed float
formatting, no timestamps embedded). Enables cross-platform hash equality in
CI and reproducible pipelines.

## CLI & distribution

**D-24 Exit codes 0/1/2/3/4** (success/conversion/usage/validation/
unsupported) — scriptable pipelines.

**D-25 Progress only on TTY, honor NO_COLOR.** Machine consumers get clean
stdout; `--report` JSON is the canonical machine channel.

**D-26 GUI = drag-and-drop shim later (optional feature flag), never
required for conversion.** Core stays headless.

## Documentation policy

**D-27 README documents the target interface; docs/STATUS.md is the honest
capability ledger and must be updated in the same PR that changes any
feature's status.** Prevents doc drift — the failure mode this project cares
most about.
