# Project status (v0.1.0 — pre-release)

This file is the **ground truth** for what actually works in this repository
today, as opposed to the target design in README.md. Update it with every
milestone PR. Legend: ✅ done · 🟡 partial · 🚧 planned/not started.

## Milestones

| # | Milestone | Status | Notes |
|---|---|---|---|
| M1 | PMX reader / writer / validator | ✅ core complete | Reader panic-free w/ random-byte test; writer deterministic + validation gate; standalone `validate` logic exists. PMD export 🚧. |
| M2 | glTF importer + mesh/skin | 🟡 import done | GLB container, accessors, skins, morph targets, VRM extensions all parsed into IR. IR→PMX vertex emission module not yet wired. |
| M3 | Materials & textures | 🚧 | PBR factors captured in IR; conversion math + `tex/` extraction not implemented. |
| M4 | Humanoid mapping + MMD skeleton + IK | 🚧 | `SemanticRole` enum covers full VRM bone set; naming heuristics and MMD bone builder not written. |
| M5 | Pose normalization (T→A rebind) | 🚧 | T/A detection heuristic exists in glTF importer; rebinding math pending. |
| M6 | Morphs | 🚧 | Targets/expressions captured in IR; name mapping + PMX morph emission pending. |
| M7 | FBX / VRM-standalone / DAE importers | 🟡 VRM via glTF | VRM 0.x/1.0 works through the glTF path. FBX & Collada importers not started. |
| M8 | Physics generation | 🚧 | Spring-bone data structures populated by VRM importer; rigid-body/joint generator pending. |
| M9 | Batch / CLI polish | 🚧 | `main.rs` is a stub; clap wiring, `--batch`, reports, `inspect` pending. |
| M10 | Docs, fuzzing, release | 🟡 | This documentation set ✅; cargo-fuzz harnesses and CI workflows pending. |

## Per-feature matrix (user-visible)

### Input formats (detection = magic bytes first)

| Format | Detect | Import | Convert end-to-end |
|---|---|---|---|
| GLB | ✅ | ✅ | 🟡 (stops before PMX emission) |
| glTF 2.0 (.gltf + external bin/textures) | ✅ | ✅ | 🟡 |
| VRM 0.x / 1.0 | ✅ | ✅ (roles, meta, expressions, spring bones) | 🟡 |
| FBX binary/ASCII | 🚧 | 🚧 | 🚧 |
| Collada .dae | 🚧 | 🚧 | 🚧 |
| PMX 2.0/2.1 | ✅ | ✅ reader | 🟡 re-export needs IR bridge |
| PMD | 🚧 | 🚧 | 🚧 |
| OBJ / STL / PLY | 🚧 | 🚧 | 🚧 (will be mesh-only + warning) |
| .blend / USD / 3DS / X (feature-flagged) | 🚧 | 🚧 | 🚧 |

### Pipeline capabilities

| Capability | Status |
|---|---|
| Coordinate/handedness conversion + winding flip | 🚧 (IR models it; normalizer unwritten) |
| Scale normalization (≈19.75 units @ 1.58 m) | 🚧 |
| Weight truncation >4 influences w/ report | ✅ helper in `importers/common.rs` |
| BDEF1/2/4 selection | ✅ in model/writer |
| SDEF | 🟡 writes zero C0/C/R params — lossy |
| QDEF (auto-bumps to PMX 2.1) | ✅ model support; producer 🚧 |
| Vertex dedup w/ skin-weight signature | ✅ helper |
| N-gon triangulation | ✅ helper |
| Standard MMD bone set + Japanese names | 🚧 |
| Leg/toe IK chains | 🚧 |
| Twist / 肩P / D-bones | 🚧 |
| Bone-map TOML overrides | 🚧 (schema documented in BONE_MAPPING.md) |
| PBR→MMD material conversion | 🚧 |
| Texture decode/re-encode (`tex/`) | 🚧 (image crate configured) |
| Toon ramps / sphere maps | 🚧 |
| Vertex/UV/bone/material morph types | ✅ model+writer; producers 🚧 |
| VRM expression → MMD morph name mapping | 🚧 |
| Rigid bodies / joints from spring bones | 🚧 |
| Metadata/license into comments | 🟡 captured in IR; writer stage 🚧 |
| Round-trip tests (write→read→compare) | 🚧 (harness planned in tests/) |
| three.js MMDLoader cross-check in CI | 🚧 |
| Fuzz corpus for readers | 🟡 random-bytes unit test only |
| Deterministic byte-identical output | ✅ by design (IndexMap + WriterOpts::auto); CI hash matrix 🚧 |

### CLI surface

| Command / flag | Status |
|---|---|
| `mmdconv <input>` convert flow | 🚧 stub binary today |
| `validate` | 🚧 subcommand wiring (library ✅) |
| `inspect` | 🚧 |
| `--scale/--height/--pose/--physics/...` | 🚧 parsing wired with M9 |
| `--batch --jobs` | 🚧 |
| `--report JSON` | 🚧 (schema in docs/CLI.md) |

## Known correctness risks (open)

1. SDEF parameters are placeholders (see above).
2. Extra-UV morph base-channel reference isn't preserved on re-export.
3. No independent-parser verification yet until the MMDLoader CI job lands.
4. The workspace does not currently compile end-to-end: `lib.rs` module
   wiring and the stub CLI must be reconciled before M2 completion. Track in
   issue "M1/M2 integration".

## What "done" means before a public v0.1 tag

* `cargo test --workspace` green on all four CI OS targets.
* Golden GLB/VRM sample converts → validates → loads in PMXEditor smoke test.
* clippy `-D warnings`, fmt check, `cargo audit` clean.
* STATUS.md updated so no ✅ claims something untested.
