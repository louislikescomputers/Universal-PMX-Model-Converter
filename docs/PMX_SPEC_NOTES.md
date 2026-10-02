# PMX spec notes

How `mmdconv` implements the PMX 2.0/2.1 binary format, and every place we
deviate from "naive" readings of the community spec. The writer is
byte-deterministic: same IR + same flags ⇒ identical file bytes on every
platform (verified in CI by hash comparison across Windows/Linux/macOS).

## Header & globals

```text
"PMX " | u8 version (2.0 / 2.1) | u8 encoding | u8 additional_count |
u8* additional sizes (must be exactly 8 for 2.0, 9 for 2.1) |
7 or 8 index sizes | model name JP/EN | comment JP/EN
```

* Little-endian throughout; no padding bytes anywhere.
* Encoding default **UTF-16LE** (flag byte `1`) — safest for Japanese names
  and what MMD itself produces. `--encoding utf8` writes flag `0`.
* Text fields are written as: `i32 byte-length (including terminator)` +
  payload + terminator (`\0\0` for UTF-16, `\0` for UTF-8). Empty strings
  write length `0` with no terminator, per convention.
* Names containing characters illegal in the chosen encoding are sanitized
  (see [../BONE_MAPPING.md](../BONE_MAPPING.md#name-sanitization)).

## Index sizes

Chosen at write time from actual counts (`WriterOpts::auto`), following
PMXEditor's observed behavior:

| Field | U8 | U16 | U32 |
|---|---|---|---|
| vertex | — | < 0x7FFF | else |
| texture | — | < 0x7FFF | else |
| material | < 0x7F | else | — |
| bone | — | < 0x7FFF | else |
| morph | — | < 0x7FFF | else |
| rigid body | — | < 0x7FFF | else |
| toon | special: 0 = none, 1 = shared ramp (U8 index), 2 = texture (vertex-size index) | | |

Negative indices (`-1`) mean "none" for optional references (parent bone,
morph group target, joint rigid bodies); the validator rejects out-of-range
non-negative values.

## Vertices & skinning

* Deform types emitted: **BDEF1, BDEF2, BDEF4, SDEF, QDEF**.
  * Chosen as the smallest sufficient type from normalized weights.
  * Weights are renormalized to sum 1.0 ± 1e-6 before writing; >4-influence
    vertices keep the 4 strongest (count reported).
  * ⚠️ Current limitation: SDEF control points (`C0`, `C`, `R0`, `R1`) are
    written as zeros when a source forces SDEF — lossy until the C0/C/R
    solver lands (tracked in STATUS.md). QDEF requires PMX 2.1.
* Edge scale is written as `1.0` unless `--edge off` (which clears the edge
  flag but keeps the field).
* UV1..UV4 supported; only UV0 is populated today, extra-UV *morphs* carry
  their base-UV reference index.

## Materials

PBR → MMD mapping (details in DECISIONS.md #D-12):

| PMX field | Source |
|---|---|
| diffuse RGBA | baseColor factor × texture; alpha from `alphaMode` (`MASK`→threshold baked into texture alpha warning; `BLEND`→diffuse alpha + self-shadow off) |
| specular RGB / shininess | derived: shininess = clamp(2..~50) from metallicRoughness roughness; specular tint from roughness/metallic |
| ambient | diffuse × configurable factor + emissive contribution |
| double-sided | `doubleSided` flag or alpha masking needs |
| toon | shared ramp (`toon0N.bmp`) via `--toon`, or per-material texture mode |
| sphere | none by default; `KHR_materials_clearcoat` can emulate multiply/additional sphere via config |

Face winding: when the handedness conversion mirrors an axis (RH→LH), triangle
index order is reversed so front faces stay front-facing in MMD. Normals and
tangents are mirrored with the same matrix.

## Bones

Flags written per spec bits: `可旋转 可移动 可见 连接 IK`(0x01–0x08…), plus
*rotation inherit*, *location inherit* (append/direct), *fixed axis*,
*local axes*, *after-physics transform*, *external parent*. The standard MMD
bone set produced by the skeleton stage is documented in
[../BONE_MAPPING.md](../BONE_MAPPING.md).

IK blocks: `IK target bone | link count | links(bone, limit on/off, low, high)`.
Knee links get X-axis-only limits with minimum angle > 0; loop count 7-ish,
angle limit ≈ 3.14 rad — values validated by our own reader tests.

## Morphs

Vertex / UV(×4 channels) / bone / impulse / material / flip / group all
representable in the data model. Panel assignment uses the reserved indices
(`system_reserve` 0–9, user panels 10+). Name-mapping tables live in
BONE_MAPPING.md §Morphs.

## Rigid bodies & joints

Shape types sphere/box/capsule; mode 0 static / 1 bone-follow / 2 bone+physics.
Joints are constrained point-to-point six-dof setups tuned for MMD's ODE fork
(conservative damping; collision groups avoid self-collision).

## Reader & validator conformance

Our reader accepts files from PMXEditor, MMD samples, and other exporters
(2.0 and 2.1, either encoding, any legal index-size combination) and is
panic-free on malformed input (bounds-checked cursor everywhere; fuzz corpus
in `tests/fixtures/fuzz`). The validator enforces the acceptance list from
the project charter: header, counts vs. index sizes, index ranges, parent
cycles, IK integrity, weight sums, face bounds, texture path existence,
morph offsets, rigid/joint references, NaN rejection.

## Cross-checkers used during development

* three.js `MMDLoader` (Node script in CI) — parses output geometry/skeleton.
* PMXEditor 2.1 / MMD 9.32 manual smoke loads for release candidates.
