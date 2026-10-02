//! Format-agnostic intermediate representation (IR).
//!
//! Every importer produces an [`IrModel`]; every conversion stage consumes and
//! updates it; the PMX writer is the only consumer for output. All geometry is
//! stored in **source space** with a unit/scale/handedness note attached — the
//! normalization stage converts to MMD space (left-handed, Y-up, 1 unit ≈ 8cm).

use glam::{Mat4, Vec3};
use indexmap::IndexMap;
use std::path::PathBuf;

pub type VertexId = u32;
pub type BoneId = u32;
pub type MeshId = u32;
pub type MaterialId = u32;
pub type TextureId = u32;
pub type MorphId = u32;

/// A named text field supporting the bilingual (JP/EN) pattern used by VRM/glTF/PMX.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LText {
    pub jp: String,
    pub en: String,
}

impl LText {
    pub fn jp<S: Into<String>>(s: S) -> Self {
        LText { jp: s.into(), en: String::new() }
    }
    pub fn both(jp: impl Into<String>, en: impl Into<String>) -> Self {
        LText { jp: jp.into(), en: en.into() }
    }
    /// Best display name: prefer non-empty JP, else EN.
    pub fn best(&self) -> &str {
        if !self.jp.is_empty() { &self.jp } else { &self.en }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpAxis {
    Y,
    Z,
}

/// Handedness + up-axis of source-space coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoordSystem {
    /// right-handed, Y up (glTF, most FBX exports)
    RhYup,
    /// right-handed, Z up (Blender/FBX defaults, Collada spec default)
    RhZup,
    /// left-handed, Y up (Unity FBX exports, PMX itself)
    LhYup,
    /// left-handed, Z up (rare; Collada with up_axis="Z_UP" + LH tools)
    LhZup,
}

impl CoordSystem {
    pub fn is_left_handed(self) -> bool {
        matches!(self, CoordSystem::LhYup | CoordSystem::LhZup)
    }
    pub fn up(self) -> UpAxis {
        match self {
            CoordSystem::RhYup | CoordSystem::LhYup => UpAxis::Y,
            CoordSystem::RhZup | CoordSystem::LhZup => UpAxis::Z,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weight {
    pub bone: BoneId,
    pub weight: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vertex {
    pub pos: Vec3,
    pub normal: Vec3,
    pub uv: [f32; 2],
    pub uv1: Option<[f32; 2]>,
    /// Sorted strongest-first, at most 4 after normalization.
    pub weights: Vec<Weight>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlphaMode {
    Opaque,
    Mask(f32),
    Blend,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Material {
    pub name: LText,
    pub pbr: PbrFactor,
    pub alpha: AlphaMode,
    pub double_sided: bool,
    pub emissive_texture: Option<TextureId>,
    /// KHR_materials_* extensions we understood (else recorded as warnings upstream).
    pub extras_ignored: Vec<String>,
}

/// Physically-based material factors (textures referenced by id).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PbrFactor {
    pub base_color: [f32; 4],
    pub base_color_texture: Option<TextureId>,
    pub metallic: f32,
    pub roughness: f32,
    pub metallic_roughness_texture: Option<TextureId>,
    pub emissive: [f32; 3],
    pub emissive_strength: f32,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Primitive {
    pub material: MaterialId,
    /// Triangle list, indices into `Mesh::vertices`.
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mesh {
    pub name: String,
    pub vertices: Vec<Vertex>,
    pub primitives: Vec<Primitive>,
    /// Inverse bind matrices, parallel to the skeleton's bone list.
    pub inverse_bind: Vec<Mat4>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticRole {
    Unknown,
    Root,
    Hips,
    Spine,
    Spine1,
    Spine2,
    Chest,
    Neck,
    Head,
    LeftEye,
    RightEye,
    Jaw,
    LeftShoulder,
    LeftUpperArm,
    LeftLowerArm,
    LeftHand,
    RightShoulder,
    RightUpperArm,
    RightLowerArm,
    RightHand,
    LeftThumbProximal,
    LeftThumbIntermediate,
    LeftThumbDistal,
    LeftIndexProximal,
    LeftIndexIntermediate,
    LeftIndexDistal,
    LeftMiddleProximal,
    LeftMiddleIntermediate,
    LeftMiddleDistal,
    LeftRingProximal,
    LeftRingIntermediate,
    LeftRingDistal,
    LeftLittleProximal,
    LeftLittleIntermediate,
    LeftLittleDistal,
    RightThumbProximal,
    RightThumbIntermediate,
    RightThumbDistal,
    RightIndexProximal,
    RightIndexIntermediate,
    RightIndexDistal,
    RightMiddleProximal,
    RightMiddleIntermediate,
    RightMiddleDistal,
    RightRingProximal,
    RightRingIntermediate,
    RightRingDistal,
    RightLittleProximal,
    RightLittleIntermediate,
    RightLittleDistal,
    LeftUpLeg,
    LeftLeg,
    LeftFoot,
    LeftToeBase,
    RightUpLeg,
    RightLeg,
    RightFoot,
    RightToeBase,
}

impl SemanticRole {
    pub fn is_left(self) -> bool {
        let s = format!("{self:?}");
        s.starts_with("Left")
    }
    pub fn is_right(self) -> bool {
        format!("{self:?}").starts_with("Right")
    }
    pub fn finger_group(self) -> Option<(&'static str, u8)> {
        // returns (finger key, segment index 0..=2) — keys: thumb,index,middle,ring,little
        let s = format!("{self:?}");
        let rest = s.trim_start_matches("Left").trim_start_matches("Right");
        let (key, seg) = match rest {
            "ThumbProximal" => ("thumb", 0),
            "ThumbIntermediate" => ("thumb", 1),
            "ThumbDistal" => ("thumb", 2),
            "IndexProximal" => ("index", 0),
            "IndexIntermediate" => ("index", 1),
            "IndexDistal" => ("index", 2),
            "MiddleProximal" => ("middle", 0),
            "MiddleIntermediate" => ("middle", 1),
            "MiddleDistal" => ("middle", 2),
            "RingProximal" => ("ring", 0),
            "RingIntermediate" => ("ring", 1),
            "RingDistal" => ("ring", 2),
            "LittleProximal" => ("little", 0),
            "LittleIntermediate" => ("little", 1),
            "LittleDistal" => ("little", 2),
            _ => return None,
        };
        Some((key, seg))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoneKind {
    Joint,
    Node,
}

#[derive(Clone, Debug)]
pub struct Bone {
    pub id: BoneId,
    pub name: LText,
    pub parent: Option<BoneId>,
    pub children: Vec<BoneId>,
    /// Local TRS relative to parent (rest pose).
    pub translation: Vec3,
    pub rotation: glam::Quat,
    pub scale: Vec3,
    /// Rest global transform (computed by `update_global_transforms`).
    pub global: Mat4,
    pub kind: BoneKind,
    pub semantic: SemanticRole,
    /// How confident the semantic assignment is.
    pub semantic_confidence: f32,
    /// Source-specific hints ("mixamo", "vrm:Head", "rigify:forearm.L", ...).
    pub tags: Vec<String>,
}

impl Bone {
    pub fn new(id: BoneId, name: LText) -> Self {
        Bone {
            id,
            name,
            parent: None,
            children: Vec::new(),
            translation: Vec3::ZERO,
            rotation: glam::Quat::IDENTITY,
            scale: Vec3::ONE,
            global: Mat4::IDENTITY,
            kind: BoneKind::Joint,
            semantic: SemanticRole::Unknown,
            semantic_confidence: 0.0,
            tags: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skeleton {
    /// Topologically ordered: parents before children.
    pub bones: Vec<Bone>,
    pub roots: Vec<BoneId>,
}

impl Skeleton {
    pub fn update_global_transforms(&mut self) {
        // bones are topologically sorted by construction; do one pass
        for i in 0..self.bones.len() {
            let local = {
                let b = &self.bones[i];
                (b.translation, b.rotation, b.scale)
            };
            let parent_global = match self.bones[i].parent {
                Some(p) => self.bones[p as usize].global,
                None => Mat4::IDENTITY,
            };
            let g = parent_global
                * Mat4::from_scale_rotation_translation(local.2, local.1, local.0);
            self.bones[i].global = g;
        }
    }

    pub fn assert_topological(&self) -> Result<()> {
        for b in &self.bones {
            if let Some(p) = b.parent {
                if p >= b.id {
                    return Err(crate::error::MmdconvError::Input(format!(
                        "skeleton not topologically sorted: bone {} has parent {}",
                        b.id, p
                    )));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexUsage {
    Color,
    Normal,
    MetallicRoughness,
    Occlusion,
    Emissive,
    Other,
}

#[derive(Clone, Debug)]
pub struct Texture {
    pub id: TextureId,
    pub name: String,
    pub usage: TexUsage,
    /// Decoded RGBA8 pixels.
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub srgb: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphTargetKind {
    Vertex,
    Uv,
    Bone,
    Material,
}

#[derive(Clone, Debug)]
pub struct MorphOffset {
    pub vertex: VertexId,
    pub offset: Vec3,
}

#[derive(Clone, Debug)]
pub struct MorphTarget {
    pub id: MorphId,
    pub name: LText,
    pub kind: MorphTargetKind,
    /// Which mesh the vertex ids reference.
    pub mesh: MeshId,
    pub offsets: Vec<MorphOffset>,
    /// Source category hint (VRM expression name, ARKit name, etc.)
    pub source_hint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColliderShape {
    Sphere,
    Box,
    Capsule,
}

#[derive(Clone, Debug)]
pub struct RigidBodyDesc {
    pub name: String,
    pub bone: Option<BoneId>,
    pub shape: ColliderShape,
    pub mass: f32,
    pub position: Vec3,
    pub rotation: glam::Quat,
    pub size: Vec3,
    pub group: u8,
    pub mask: u16,
    pub kinematic: bool,
    pub gravity: bool,
}

#[derive(Clone, Debug)]
pub struct SpringBoneCollider {
    pub bone: BoneId,
    pub offset: Vec3,
    pub radius: f32,
    pub length: f32,
}

#[derive(Clone, Debug)]
pub struct SpringBone {
    pub joint: BoneId,
    pub center: Option<BoneId>,
    pub axis: Vec3,
    pub gravity_power: f32,
    pub stiffness: f32,
    pub hit_radius_scale: f32,
    pub pull_back: f32,
    pub front_limit: f32,
    pub rear_limit: f32,
    pub left_limit: f32,
    pub right_limit: f32,
    pub down_limit: f32,
    pub up_limit: f32,
    pub colliders: Vec<SpringBoneCollider>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoseKind {
    Unknown,
    TPose,
    APose,
}

#[derive(Clone, Debug, Default)]
pub struct ModelMeta {
    pub name: LText,
    pub comment: LText,
    pub author: String,
    pub license: String,
    pub source_file: PathBuf,
}

/// The complete format-agnostic model.
#[derive(Clone, Debug)]
pub struct IrModel {
    pub meta: ModelMeta,
    pub coord: CoordSystem,
    /// meters per source unit (authoritative when known; importers estimate otherwise)
    pub units_to_meters: Option<f32>,
    pub pose: PoseKind,
    pub skeleton: Skeleton,
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub morphs: Vec<MorphTarget>,
    pub rigid_bodies: Vec<RigidBodyDesc>,
    pub springs: Vec<SpringBone>,
    /// Nodes that carry whole-mesh transforms (for unskinned rigid binding).
    pub mesh_attachments: Vec<(MeshId, BoneId)>,
    pub humanoid_map_present: bool,
}

impl IrModel {
    pub fn empty() -> Self {
        IrModel {
            meta: ModelMeta::default(),
            coord: CoordSystem::RhYup,
            units_to_meters: None,
            pose: PoseKind::Unknown,
            skeleton: Skeleton::default(),
            meshes: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            morphs: Vec::new(),
            rigid_bodies: Vec::new(),
            springs: Vec::new(),
            mesh_attachments: Vec::new(),
            humanoid_map_present: false,
        }
    }

    pub fn total_vertices(&self) -> usize {
        self.meshes.iter().map(|m| m.vertices.len()).sum()
    }
    pub fn total_triangles(&self) -> usize {
        self.meshes
            .iter()
            .flat_map(|m| m.primitives.iter())
            .map(|p| p.indices.len() / 3)
            .sum()
    }

    /// Gather all bones referenced by skin weights.
    pub fn skinned_bones(&self) -> IndexMap<BoneId, ()> {
        let mut set: IndexMap<BoneId, ()> = IndexMap::new();
        for m in &self.meshes {
            for v in &m.vertices {
                for w in &v.weights {
                    set.entry(w.bone).or_insert(());
                }
            }
        }
        set
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ltext_best_prefers_jp() {
        assert_eq!(LText::both("", "B").best(), "B");
        assert_eq!(LText::both("A", "B").best(), "A");
    }

    #[test]
    fn global_transform_chain() {
        let mut sk = Skeleton::default();
        let mut a = Bone::new(0, LText::jp("a"));
        a.translation = Vec3::Y;
        let mut b = Bone::new(1, LText::jp("b"));
        b.parent = Some(0);
        b.translation = Vec3::X;
        sk.bones.push(a);
        sk.bones.push(b);
        sk.roots.push(0);
        sk.update_global_transforms();
        assert_eq!(sk.bones[1].global.transform_point3(Vec3::ZERO), Vec3::new(1.0, 1.0, 0.0));
    }

    #[test]
    fn topo_check_rejects_bad_parent() {
        let mut sk = Skeleton::default();
        let mut a = Bone::new(0, LText::jp("a"));
        a.parent = Some(1);
        sk.bones.push(a);
        sk.bones.push(Bone::new(1, LText::jp("b")));
        assert!(sk.assert_topological().is_err());
    }

    #[test]
    fn finger_group_mapping() {
        assert_eq!(SemanticRole::LeftRingIntermediate.finger_group(), Some(("ring", 1)));
        assert_eq!(SemanticRole::Head.finger_group(), None);
        assert!(SemanticRole::RightLeg.is_right());
    }
}
