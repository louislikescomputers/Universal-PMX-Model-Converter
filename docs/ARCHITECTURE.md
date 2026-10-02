# Architecture

`mmdconv` is a cargo workspace with two crates and a strictly layered,
stage-based pipeline. Every stage is a separate module with unit tests, and
all stages communicate through one format-agnostic **intermediate
representation (IR)**.

```
crates/
  mmdconv-core/          # library: everything except argument parsing
    src/
      lib.rs             # crate wiring
      error.rs           # MmdconvError + human fix-hints
      ir.rs              # the intermediate representation
      importers/
        mod.rs           # magic-byte format detection + dispatch
        common.rs        # triangulation, dedup, weight normalization helpers
        gltf.rs          # GLB container + glTF 2.0 + VRM extension importer
        fbx.rs           # 🚧 planned (M7)
        dae.rs           # 🚧 planned (M7)
        pmximport.rs     # 🚧 PMX/PMD → IR (re-export path)
      normalize.rs       # 🚧 units / axes / handedness / pose (M5)
      skeleton/
        mapping.rs       # 🚧 humanoid mapping heuristics (M4)
        mmd_bones.rs     # 🚧 standard MMD bone set, IK, helpers (M4)
      mesh.rs            # 🚧 mesh/skin → PMX vertices (M2 tail)
      materials.rs       # 🚧 PBR → MMD + texture extraction (M3)
      morphs.rs          # 🚧 blendshapes/expressions → PMX morphs (M6)
      physics.rs         # 🚧 spring bones → rigid bodies/joints (M8)
      pmx/
        model.rs         # complete PMX 2.0/2.1 data model ✅
        writer.rs        # deterministic spec-exact writer ✅
        reader.rs        # bounds-checked parser (never panics) ✅
        validate.rs      # standalone validator ✅
        mod.rs           # index-size recomputation for cleanup path ✅
  mmdconv-cli/           # `mmdconv` binary: clap arg parsing, batch, report
    src/main.rs
```

## Data flow

```
input file
   │  detect_format() — magic bytes first, extension only as tie-breaker
   ▼
Importer ──────────────► IrModel (source space, source units)
                         nodes+bones, meshes+skins, materials+textures,
                         morph targets, spring bones, metadata/license
   ▼
Normalize              units→cm-ish scale, RH→LH mirror (flip winding &
                       normals), up-axis → Y, pose T/A/keep rebind
   ▼
Skeleton analysis      role detection priority:
                       1. VRM humanoid map   2. naming conventions
                       (Unity/Mixamo/Rigify/UE, .L/.R, _l/_r)
                       3. geometry/topology heuristics (chains, symmetry)
   ▼
MMD skeleton build     全ての親…足先EX, IK chains, twist/D-bones, extras
   ▼
Mesh/Skin, Material/Texture, Morph, Physics converters (each IR→PmxModel)
   ▼
PmxModel ──► validate() ──► write_pmx() ──► output.pmx + tex/*.png
                ▲
                └── same validator runs post-write via read_pmx() in tests
```

## The IR (`ir.rs`)

Key design decisions:

* **Source-space storage.** Importers never flip axes; they record a
  `CoordSystem` enum (`RhYup`, `RhZup`, `LhYup`, `LhZup`). Normalization is
  one explicit matrix pass, which keeps importers simple and testable.
* **Unlimited influences until skinning.** Vertices carry `Vec<Weight>`; the
  "keep top-4, renormalize" reduction happens in `importers/common.rs` with a
  truncation counter that surfaces in the JSON report.
* **Semantic bone roles** (`SemanticRole`) cover the full VRM humanoid set
  (65 roles incl. finger segments `親指０`…`小指３`) plus left/right queries
  and finger-group helpers used by the MMD bone builder.
* **Bilingual text** (`LText { jp, en }`) matches how PMX/VRM/glTF all store
  names, so nothing is lost on re-export.
* **Physics descriptors** (`SpringBone`, `SpringBoneCollider`,
  `RigidBodyDesc`, `ColliderShape`) are populated by the VRM importer today
  and consumed by the (planned) physics stage without format coupling.
* Determinism: iteration order uses `IndexMap` everywhere it can affect
  output bytes.

## PMX layer

The PMX module is deliberately independent of the IR so it doubles as a
standalone library (`read_pmx`, `write_pmx`, `validate`) powering:

1. the conversion output path,
2. `mmdconv validate`,
3. PMX→PMX cleanup/re-export,
4. round-trip tests (writer → reader → structural compare vs. IR).

Index sizes (`IdSize::U8/U16/U32`) are computed from actual counts at write
time (`WriterOpts::auto`), matching PMXEditor's conventions. The writer
refuses to emit an unvalidated model — validation is a hard gate, not a lint.

## Error handling philosophy

* Library code returns `Result<T, MmdconvError>`; parsers are bounds-checked
  and **cannot panic on malformed input** (fuzz-tested with random-byte
  streams).
* The CLI renders errors with *fix hints* (`error::describe_with_hint`) —
  e.g. unsupported formats list accepted extensions, missing files warn about
  full-width characters in Japanese filenames.
* Every lossy step emits a warning into the report; silent degradation is a
  bug.

## Parallelism & performance

`rayon` parallelizes per-file work in `--batch`, texture decode/re-encode,
and vertex dedup. Target envelope: 500k+ vertices, 500+ bones, 100+
materials; measured numbers will live in the release notes once benchmarks
exist (see docs/STATUS.md).
