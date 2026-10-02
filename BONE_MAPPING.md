# BONE_MAPPING.md — mapping tables, heuristics, and the `--bone-map` schema

How a source skeleton becomes an MMD skeleton. Implementation lives in
`crates/mmdconv-core/src/skeleton/` (see docs/STATUS.md for progress); this
document is the normative reference both for users and for that code.

## 1. Mapping priority

For each standard MMD bone, the first matching rule wins:

1. **VRM humanoid** — `humanoid.humanBones[]` role → MMD bone (table §3).
2. **Known rig naming conventions** — Unity Humanoid, Mixamo, Rigify,
   UE Mannequin, common DAZ/SMPL names (tables §4–§6).
3. **Generic left/right + semantic pattern matching** (§7): prefixes/suffixes
   `L/R`, `.L/.R`, `_l/_r`, `-L/-R`, JP `左/右`; keywords `hip|upperleg|knee|
   shin|ankle|foot|toe|clavicle|shoulder|upperarm|lowerarm|hand|finger…`.
4. **Geometry/topology heuristics** (§8): limb-chain detection, bilateral
   symmetry about the sagittal plane, relative bone lengths, height ordering.
5. Failure ⇒ model is treated as non-humanoid (generic path, §9) or bones
   become extras (§10).

## 2. Standard MMD output skeleton (JP names)

Root chain: `全ての親` → `センター` → (`グルーブ`) → `腰` → `上半身` →
`上半身2` → `首` → `頭`, plus `両目`, `左目`, `右目`.

Arms (per side): `肩`(or 肩P helper) → `腕` → `ひじ` → `手首`, optional twist
bones `腕捩`(1–3) & `手捩`(1–3), shoulder-P `肩P`. Fingers per hand:

| Hand finger | PMX bone names |
|---|---|
| Thumb | 親指０, 親指１, 親指２ |
| Index | 人指１, 人指２, 人指３ |
| Middle | 中指１, 中指２, 中指３ |
| Ring | 薬指１, 薬指２, 薬指３ |
| Pinky | 小指１, 小指２, 小指３ |

Legs (per side): `足` → `ひざ` → `足首` → `足先EX`, optional D-bones
`足D`, `ひざD`, `足首D`. IK chains (unless `--no-ik`):

* `左足ＩＫ` / `右足ＩＫ`: target = 足IK親→…→ links [足, ひざ, 足首],
  knee link limited to X-axis with min angle > 0 (anti-hyperextension,
  DECISIONS D-11), loop count 7, angle limit π.
* `左つま先ＩＫ` / `右つま先ＩＫ`: links [足首, 足先EX].
* IK parent = センター (or グルーブ when enabled); `足IK親` receives the
  foot bone's location inherit-append.

Helper bones are only created when the source geometry supports them
(twist requires separate twist bones or sufficiently long limbs; toggled by
`--no-helper-bones`).

## 3. VRM humanoid role → MMD bone

VRM 1.0 bone roles (0.x names differ only in casing) map 1:1 onto the IR
`SemanticRole` enum already implemented in `ir.rs`:

| VRM role | MMD bone | VRM role | MMD bone |
|---|---|---|---|
| hips | 腰 | leftShoulder | 左肩 |
| spine | センター/上半身 | leftUpperArm | 左腕 |
| chest | 上半身2 | leftLowerArm | 左ひじ |
| upperChest | 上半身2* | leftHand | 左手首 |
| neck | 首 | fingers L/R: thumbMetacarpal→親指０ … pinkyPhalanx3→小指３ |
| head | 頭 | legs: upperLeg→左脚, lowerLeg→左ひざ, foot→左足首, toe→左足先EX, platform→足IK親 |
| eyes L/R | 左目/右目 | jaw | (unused) |
| (bothEyes node) | 両目 | | |

\* When both `chest` and `upperChest` exist, 上半身 anchors at chest and
上半身2 at upperChest.

## 4. Unity Humanoid / generic muscle names

`Hips→腰, Spine→センター, Chest→上半身, UpperChest→上半身2, Neck→首,
Head→頭, LeftShoulder→左肩, LeftUpperArm→左腕, LeftLowerArm→左ひじ,
LeftHand→左手首, LeftThumbProximal/Middle/Distal→親指０..２ (proximal maps
to ０ only when a metacarpal exists, else １), LeftIndex…Little 1/2/3→
人指/中指/薬指/小指１..３, LeftUpperLeg→左足, LeftLowerLeg→左ひざ,
LeftFoot→左足首, LeftToes→左足先EX` (mirror for Right).

## 5. Mixamo

Mixamo FBX/GLB names: `mixamorig:Hips, Spine, Spine1, Spine2, Neck, Head,
LeftShoulder, LeftArm, LeftForeArm, LeftHand, LeftHandIndex1..3 (+Thumb0..2,
Middle, Ring, Pinky), LeftUpLeg, LeftLeg, LeftFoot, LeftToeBase`.
Mapping: Hips→腰, Spine→センター, Spine1→上半身, Spine2→上半身2, Arm→腕,
ForeArm→ひじ, Hand→手首, UpLeg→足, Leg→ひざ, Foot→足首, ToeBase→足先EX,
Pinkies→小指, thumbs 0-based shift handled like §4. The `mixamorig:` prefix
is stripped before matching.

## 6. Rigify & UE Mannequin

* **Rigify** (`.L`/`.R` suffix convention): `torso→センター, spine→上半身,
  spine.003→上半身2, neck→首, head→頭, shoulder.L→左肩, upper_arm.L→左腕,
  forearm.L→左ひじ, hand.L→左手首, thigh.L→左足, shin.L→左ひざ, foot.L→
  左足首, toe.L→左足先EX, face.B-… ignored unless weights exist; finger
  bones `f_index.01..03.L` etc. map by digit keyword.`
* **UE Mannequin** (`thigh_l, calf_l, foot_l, ball_l, upperarm_l,
  lowerarm_l, hand_l, clavicle_l, pelvis, spine_01..03, neck_01, head,
  finger1_01_l…`): ball→足先EX, pelvis→腰, spine_01/02/03→センター/上半身/
  上半身2, clavicle→肩, fingerN where 1=thumb…5=pinky, three phalanges each.

## 7. Generic name patterns

Normalization pipeline before matching: NFKC Unicode normalize, strip rig
prefixes (`mixamorig:`, `bip001_`, `Bone_`, `root/`), case-fold, split on
`_ . - :`, isolate trailing/leading side tokens (`l r left right .l .r _l _r
-L -R 左 右`). Side defaults: token present → that side; absent → inferred
by world-X sign of the head position (§8).

## 8. Geometry heuristics (last resort)

1. Find root = deepest ancestor of ≥50% of weighted bones.
2. Chain detection: walk longest descendant paths; classify by length ratios
   against total height (upper leg ≈ 0.25h, lower leg ≈ 0.25h, arm segments
   ≈ 0.17h/0.14h).
3. Bilateral symmetry: pair bones whose rest positions mirror across the
   sagittal plane within tolerance and share parent+child structure; the one
   with +X head position is "left" in RH sources (flipped for LH).
4. Ambiguity ⇒ don't guess: report bone as unmapped extra instead of a wrong
   standard bone. Precision beats recall here — a mis-mapped 左ひざ breaks IK.

## 9. Non-humanoid fallback

If fewer than {hips, spine-or-chest, head} OR no leg/arm pairs resolve, the
model takes the generic path: original hierarchy preserved under new
`全ての親` + `センター`, **no IK, no standard-name mapping**, warning emitted
(`non-humanoid skeleton: kept original hierarchy, no IK generated`).

## 10. Extra bones

Hair, skirt, tail, accessories, cloth bones stay in the output: names
sanitized (§11), deduplicated by appending `#2`…, attached to the nearest
mapped ancestor (weighted majority of skin usage if determinable). Pruned
only if they weight zero vertices and `--keep-unknown-bones` is off.

## 11. Name sanitization

UTF-16 encodable chars pass through unchanged (incl. Japanese). For UTF-8
output mode, surrogates outside BMP are kept (PMX allows any UTF-8). Control
chars stripped; leading/trailing spaces trimmed; empty result → `bone_<id>`.

## 12. `--bone-map <file.toml>` schema

User overrides win over every automatic rule. Unknown keys error out (typos
must not silently no-op).

```toml
# my-bonemap.toml
version = 1

[bones]
# "source bone name (exact, after prefix stripping)" = "target MMD bone"
"Hips"          = "腰"
"Spine"         = "センター"
"Tail_01"       = "尻尾1"        # custom bone: kept with given name
"LeftEyeSocket" = "左目"

[roles]
# or address semantic roles directly (applies to whichever bone matches):
head     = "頭"
leftHand = "左手首"

[skip]
# bones excluded from output entirely (not even as extras)
patterns = ["camera.*", "joint1"]

[morphs]
# morph name remaps, same semantics as --bone-map covers morphs too
"Blink_L" = "ウィンク"
"Blink_R" = "ウィンク右"

[physics]
ignore_chains = ["tie_physics"]   # skip spring-bone chain generation here
```

Rules: `[bones]` values must be either standard MMD names (validated against
the built-in table) or new custom names; cycles rejected at load time; the
file is applied *after* automatic mapping so it can fix individual mistakes
without re-specifying the whole rig. Multiple files can be passed; later
files override earlier ones.
