# CLI reference — `mmdconv`

> Status note: the flag surface below is the **target interface**. Which
> flags are wired end-to-end in the current build is tracked in
> [STATUS.md](STATUS.md). Unimplemented flags currently exit with code 2 and
> a clear message rather than being silently ignored.

## Synopsis

```text
mmdconv <input> [-o output.pmx] [OPTIONS]
mmdconv --batch <dir> [--jobs N] [OPTIONS]
mmdconv validate <file.pmx> [--encoding auto|utf16|utf8]
mmdconv inspect  <file> [--json]
mmdconv --help | --version
```

## Commands

### Convert (default)

Reads any supported rigged model and writes a PMX. Output defaults to
`<input-stem>.pmx` beside the input; textures go to `<tex-dir>/` beside the
output (default name `tex`). Existing outputs are overwritten only with
`--force` (batch mode skips finished files automatically).

### `validate <file.pmx>`

Re-parses the file and checks: header sanity, count/index-size agreement,
index ranges (faces, bones, textures, morph offsets), bone-parent cycles
(tortoise-and-hare), IK link integrity (loop count, angle limits, valid IK
target), weight sums ≈ 1.0, rigid-body/joint cross-references, NaN/Inf
anywhere, and texture path existence + relativity. Exit 0 when clean, 3 when
problems found; each problem printed with its byte section.

### `inspect <file>`

Prints detected format, coordinate system, node/bone tree with roles, mesh
stats (verts/tris per material group), skinning summary (max influences,
truncations), material list with texture slots, morph table, spring-bone
chains, and license metadata. `--json` emits the machine-readable form.

## Options

| Option | Values | Default | Description |
|---|---|---|---|
| `-o, --output` | path | beside input | Output PMX path. Unicode/spaces OK. |
| `--scale <f>` | float > 0 | — | Multiply source units by `f`. Mutually exclusive with `--height`. |
| `--height <m>` | meters | 1.58 | Normalize total rest-pose height to `m` meters (1.58 m ≈ 19.75 MMD units). |
| `--pose` | `a` \| `t` \| `keep` | `a` | Rest-pose normalization. Skin is rebound (vertices transformed by bind matrices, inverse bind poses rebuilt) so weights stay correct. |
| `--physics` | `auto` \| `off` | `auto` | Generate rigid bodies + joints from spring/dynamic bone data. `auto` no-ops with a warning if the source has none. |
| `--physics-strength` | 0.0–4.0 | 1.0 | Scales mass/gravity of generated physics (heavier = more stable, less lively). |
| `--edge` | `on` \| `off` | `on` | Per-material outline flag + edge color. |
| `--toon` | `none` \| `1`..`10` | `2` | Shared toon ramp (`toon01.bmp` … `toon10.bmp` from MMD's `toon/` folder). |
| `--texture-format` | `png` \| `jpg` \| `tga` \| `bmp` | `png` | Re-encode target. Alpha-preserving formats keep alpha; `jpg` flattens onto white with a warning. |
| `--texture-dir` | name | `tex` | Folder beside the output for extracted textures (relative paths stored in PMX use `/`). |
| `--bone-map` | file.toml | — | User overrides for source-bone → MMD-bone mapping. Schema in [../BONE_MAPPING.md](../BONE_MAPPING.md). |
| `--pmx-version` | `2.0` \| `2.1` | `2.0` | `2.1` is selected automatically when required (QDEF, soft body); forcing `2.0` with such features errors instead of corrupting. |
| `--encoding` | `utf16` \| `utf8` | `utf16` | PMX text encoding (header flag 10). |
| `--no-ik` | — | off | Skip leg/toe IK chain generation (pure-forward-kinematics skeleton). |
| `--no-helper-bones` | — | off | Skip twist bones (腕捩/手捩), 肩P, and D-bones (足D/ひざD/足首D). |
| `--keep-unknown-bones` | — | off | Keep unmapped bones (sanitized, deduplicated names, nearest mapped parent) instead of pruning non-weighted leaves. |
| `--format` | `pmx` \| `pmd` | `pmx` | `pmd` is planned; currently exits 2 with a hint. |
| `--batch <dir>` | dir | — | Recursive scan for supported inputs; converts in parallel; skips files whose `.pmx` already exists and is newer. |
| `--jobs <N>` | int | #CPU | Batch parallelism. |
| `--report <file.json>` | path | — | JSON report (schema below). |
| `--dry-run` | — | off | Full analysis + plan printed; nothing written. |
| `--force` | — | off | Overwrite existing outputs. |
| `-v, --verbose` / `-q, --quiet` | — | — | Trace-level logging / suppress warnings. |

Progress output appears only when stdout is a TTY; colors respect `NO_COLOR`.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Conversion failed (import, mapping, or I/O error — message includes a fix hint) |
| 2 | CLI usage error (bad flag combination, unknown option, unimplemented flag) |
| 3 | Output failed validation (also possible from `validate`) |
| 4 | Input format unsupported/unrecognized |

## JSON report schema

```json
{
  "tool": { "name": "mmdconv", "version": "0.1.0" },
  "input":  { "path": "model.vrm", "format": "vrm", "coords": "RhYup" },
  "output": { "path": "model.pmx", "pmx_version": "2.0", "bytes": 123456,
               "sha256": "…", "textures": ["tex/body.png"] },
  "stats":  { "vertices": 42000, "triangles": 81000, "bones": 187,
               "materials": 12, "morphs": 46, "rigid_bodies": 0, "joints": 0 },
  "bones":  { "mapped": 52, "unmapped": 131,
               "mapping_source": { "vrm_humanoid": 52 },
               "standard_bones_created": 61, "ik_chains": 4 },
  "skinning": { "bdef1": 300, "bdef2": 12000, "bdef4": 29700,
                 "truncated_vertices": 42 },
  "pose":   { "source": "t", "target": "a", "rebound_vertices": 42000 },
  "warnings": ["KHR_materials_emissive_strength not representable in MMD; baked into ambient"],
  "timings_ms": { "import": 120, "normalize": 15, "skeleton": 8, "mesh": 210,
                    "materials": 90, "morphs": 12, "physics": 0, "write": 60,
                    "total": 515 }
}
```

In `--batch` mode the report is an object with `"results": [ … ]`, one entry
per file, plus `"skipped": [...]` and aggregate timings.

## Examples

```sh
# Simplest case
mmdconv モデル.glb                        # Unicode filename, spaces fine

# Windows paths with backslashes
mmdconv C:\Users\me\Models\rig.fbx -o D:\mmd\Model\rig.pmx

# Keep source pose, no IK, no helpers, UTF-8 encoding, custom scale
mmdconv prop.glb --pose keep --no-ik --no-helper-bones --encoding utf8 --scale 100

# Convert everything under a folder using 8 threads and a shared bone-map
mmdconv --batch ./incoming --jobs 8 --bone-map team-bonemap.toml --report out.json

# Verify a file before shipping it
mmdconv validate final.pmx && echo OK
```
