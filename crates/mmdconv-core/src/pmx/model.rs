//! PMX 2.0/2.1 in-memory data model.

use glam::{Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PmxEncoding {
    Utf16Le,
    Utf8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdSize {
    U8,
    U16,
    U32,
}

impl IdSize {
    pub fn bytes(self) -> usize {
        match self {
            IdSize::U8 => 1,
            IdSize::U16 => 2,
            IdSize::U32 => 4,
        }
    }
    /// Smallest size that can hold indices `0..count` plus a "-1" sentinel.
    pub fn for_count(count: usize) -> IdSize {
        if count < 0x7F {
            IdSize::U8
        } else if count < 0x7FFF {
            IdSize::U16
        } else {
            IdSize::U32
        }
    }
}

/// A PMX text field: name (required), optional English and official-name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PmxText {
    pub name: String,
    pub en: Option<String>,
    pub original: Option<String>,
}

impl PmxText {
    pub fn new(name: impl Into<String>) -> Self {
        PmxText { name: name.into(), en: None, original: None }
    }
    pub fn with_en(mut self, en: impl Into<String>) -> Self {
        let e = en.into();
        if !e.is_empty() && e != self.name {
            self.en = Some(e);
        }
        self
    }
    pub fn both(jp: impl Into<String>, en: impl Into<String>) -> Self {
        let jp = jp.into();
        let en = en.into();
        let same = jp == en;
        PmxText { name: jp, en: if en.is_empty() || same { None } else { Some(en) }, original: None }
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PmxModel {
    pub version: f32, // 2.0 or 2.1
    pub encoding: PmxEncoding,
    pub additional_uv_count: u8,
    pub vertex_size: IdSize,
    pub texture_size: IdSize,
    pub material_size: IdSize,
    pub bone_size: IdSize,
    pub morph_size: IdSize,
    pub rigid_body_size: IdSize,
    pub toon_size: IdSize,
    pub shift_jis_replacement: bool,

    pub name: PmxText,
    pub comment: PmxText,

    pub vertices: Vec<PmxVertex>,
    pub faces: Vec<u32>, // triangle list; len % 3 == 0
    pub textures: Vec<String>,
    pub materials: Vec<PmxMaterial>,
    pub bones: Vec<PmxBone>,
    pub morphs: Vec<PmxMorph>,
    pub display_frames: Vec<PmxFrame>,
    pub soft_bodies: Vec<Vec<u8>>, // preserved raw (only for unknown 2.1 sections)
    pub rigid_bodies: Vec<PmxRigidBody>,
    pub joints: Vec<PmxJoint>,
}

impl Default for PmxModel {
    fn default() -> Self {
        PmxModel {
            version: 2.0,
            encoding: PmxEncoding::Utf16Le,
            additional_uv_count: 0,
            vertex_size: IdSize::U32,
            texture_size: IdSize::U32,
            material_size: IdSize::U32,
            bone_size: IdSize::U32,
            morph_size: IdSize::U32,
            rigid_body_size: IdSize::U32,
            toon_size: IdSize::U32,
            shift_jis_replacement: true,
            name: PmxText::new("model"),
            comment: PmxText::default(),
            vertices: Vec::new(),
            faces: Vec::new(),
            textures: Vec::new(),
            materials: Vec::new(),
            bones: Vec::new(),
            morphs: Vec::new(),
            display_frames: Vec::new(),
            soft_bodies: Vec::new(),
            rigid_bodies: Vec::new(),
            joints: Vec::new(),
        }
    }
}

impl PmxModel {
    pub fn edge_count(&self) -> usize {
        self.faces.len() / 3
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeformType {
    Bdef1,
    Bdef2,
    Bdef4,
    Sdef,
    Qdef,
}

impl DeformType {
    pub fn from_u8(v: u8) -> Option<DeformType> {
        Some(match v {
            0 => DeformType::Bdef1,
            1 => DeformType::Bdef2,
            2 => DeformType::Bdef4,
            3 => DeformType::Sdef,
            4 => DeformType::Qdef,
            _ => return None,
        })
    }
    pub fn code(self) -> u8 {
        match self {
            DeformType::Bdef1 => 0,
            DeformType::Bdef2 => 1,
            DeformType::Bdef4 => 2,
            DeformType::Sdef => 3,
            DeformType::Qdef => 4,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxVertex {
    pub pos: Vec3,
    pub normal: Vec3,
    pub uv: [f32; 2],
    pub extra_uv: Vec<[f32; 4]>,
    pub deform: DeformType,
    /// Always length 4 here; bones >= 1 unused are set to 0, weights after sum 0.
    pub bones: [u32; 4],
    pub weights: [f32; 4],
    pub edge_scale: f32,
    pub shape: u8,      // 2.1: 0 none /1 ring /2 free
    pub wire: bool,     // 2.1
    pub vert_ref: Option<u32>, // 2.1 supplemental vertex reference
}

impl PmxVertex {
    pub fn bdef(pos: Vec3, bones_weights: &[(u32, f32)]) -> PmxVertex {
        let mut v = PmxVertex {
            pos,
            normal: Vec3::Y,
            uv: [0.0; 2],
            extra_uv: Vec::new(),
            deform: DeformType::Bdef1,
            bones: [0; 4],
            weights: [0.0; 4],
            edge_scale: 1.0,
            shape: 0,
            wire: false,
            vert_ref: None,
        };
        let n = bones_weights.len().min(4);
        for i in 0..n {
            let (b, w) = bones_weights[i];
            v.bones[i] = b;
            v.weights[i] = w;
        }
        v.deform = match n {
            0 | 1 => DeformType::Bdef1,
            2 => DeformType::Bdef2,
            _ => DeformType::Bdef4,
        };
        if n <= 1 {
            v.weights[0] = 1.0;
        }
        v
    }
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub struct PmxMaterial {
    pub name: PmxText,
    pub diffuse: [f32; 4],
    pub specular: [f32; 3],
    pub shininess: f32,
    pub ambient: [f32; 3],
    pub double_sided: bool,
    pub self_shadow: bool,
    pub receive_shadow: bool,
    pub cache: bool,
    pub draw_line: bool,
    pub draw_ground_shadow: bool,
    pub draw_edge: bool,
    pub vertex_color_mode: bool,
    pub point_size: f32,
    pub line_width: f32,
    pub edge_color: [f32; 4],
    pub texture: Option<u32>,
    pub sphere: Option<u32>,
    pub sphere_mode: u8, // 0 none, 1 mul, 2 add
    pub shared_toon: bool,
    pub toon: Option<u32>,
    pub memo: String,
}

impl Default for PmxMaterial {
    fn default() -> Self {
        PmxMaterial {
            name: PmxText::new("mat"),
            diffuse: [0.9, 0.9, 0.9, 1.0],
            specular: [0.0; 3],
            shininess: 5.0,
            ambient: [0.0; 3],
            double_sided: false,
            self_shadow: true,
            receive_shadow: true,
            cache: false,
            draw_line: true,
            draw_ground_shadow: true,
            draw_edge: true,
            vertex_color_mode: false,
            point_size: 1.0,
            line_width: 1.0,
            edge_color: [0.0, 0.0, 0.0, 1.0],
            texture: None,
            sphere: None,
            sphere_mode: 0,
            shared_toon: true,
            toon: Some(0),
            memo: String::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoneTarget {
    Index(u32),
    Position(Vec3),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TailType {
    Index(u32),
    Offset(Vec3),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inherit {
    pub bone: u32,
    pub ratio: f32,
    pub mode: u8, // 0 rot+trans, 1 rot, 2 trans
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxIkLink {
    pub bone: u32,
    pub limit_enabled: bool,
    pub lower: Vec3,
    pub upper: Vec3,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxIk {
    pub target: u32,
    pub loop_count: u32,
    pub angle_limit: f32,
    pub links: Vec<PmxIkLink>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct PmxBone {
    pub name: PmxText,
    pub position: Vec3,
    pub parent: Option<u32>,
    pub transform_after_deform: bool,
    pub head_id: BoneTarget,
    pub tail: TailType,
    pub level: u32,
    pub flags: u32,
    pub inherit: Option<Inherit>,
    pub external_parent: Option<(u32, u32)>, // (bone index, type 0/1)
    pub fixed_axis: Option<Vec3>,
    pub local_x: Option<Vec3>,
    pub local_z: Option<Vec3>,
    pub after_physics: bool,
    pub external_transform: bool,
    pub visible: bool,
    pub enabled: bool,
    pub ik: Option<PmxIk>,
    pub append_local: bool,
}

impl Default for PmxBone {
    fn default() -> Self {
        PmxBone {
            name: PmxText::new("bone"),
            position: Vec3::ZERO,
            parent: None,
            transform_after_deform: false,
            head_id: BoneTarget::Position(Vec3::ZERO),
            tail: TailType::Offset(Vec3::ZERO),
            level: 0,
            flags: FLAG_ROTATABLE | FLAG_MOVABLE | FLAG_VISIBLE | FLAG_ON,
            inherit: None,
            external_parent: None,
            fixed_axis: None,
            local_x: None,
            local_z: None,
            after_physics: false,
            external_transform: false,
            visible: true,
            enabled: true,
            ik: None,
            append_local: false,
        }
    }
}

// Bone flag bits (PMX spec)
pub const FLAG_TAIL_INDEX: u32 = 0x0001;
pub const FLAG_ROTATABLE: u32 = 0x0002;
pub const FLAG_MOVABLE: u32 = 0x0004;
pub const FLAG_VISIBLE: u32 = 0x0008;
pub const FLAG_ON: u32 = 0x0010;
pub const FLAG_AFFECT_ROOT: u32 = 0x0020;
pub const FLAG_IK: u32 = 0x0040;
pub const FLAG_APPEND_ROTATE: u32 = 0x0100;
pub const FLAG_APPEND_TRANSLATE: u32 = 0x0200;
pub const FLAG_EXTERNAL_PARENT: u32 = 0x0400;
pub const FLAG_FIXED_AXIS: u32 = 0x0800;
pub const FLAG_LOCAL_AXIS: u32 = 0x1000;
pub const FLAG_AFTER_PHYSICS: u32 = 0x2000;
pub const FLAG_EXTERNAL_TRANSFORM: u32 = 0x4000;
pub const FLAG_DISABLED: u32 = 0x8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MorphPanel {
    Other = 0,
    Category = 1,
    Brow = 2,
    Mouth = 3,
    Eye = 4,
    Lip = 5,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PmxMorphData {
    Vertex(Vec<(u32, Vec3)>),
    Uv(Vec<(u32, [f32; 4])>),
    BoneRel(Vec<(u32, Vec3, Quat)>),
    Impulse(Vec<(u32, Vec3, Quat)>),
    InverseRatio(Vec<(u32, f32)>),
    Material(Vec<PmxMatMorphOff>),
    Flip(Vec<(u32, f32)>),
    Group(Vec<(u32, f32)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxMatMorphOff {
    pub material: i64, // -1 = all
    pub diffuse: Option<([f32; 4], u8)>,   // (value, mode)
    pub specular: Option<([f32; 3], u8)>,
    pub shininess: Option<(f32, u8)>,
    pub ambient: Option<([f32; 3], u8)>,
    pub edge_color: Option<([f32; 4], u8)>,
    pub edge_size: Option<(f32, u8)>,
    pub texture: Option<([f32; 4], u8)>,
    pub sphere: Option<([f32; 4], u8)>,
    pub toon: Option<([f32; 4], u8)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxMorph {
    pub name: PmxText,
    pub panel: MorphPanel,
    pub offset_kind: u8, // 0 group,1 vertex,2 bone,3 uv,4 extraUv,5 impulse,6 inverseRatio,7 material,8 flip
    pub data: PmxMorphData,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxFrameElement {
    pub is_morph: bool,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxFrame {
    pub name: PmxText,
    pub is_special: bool,
    pub elements: Vec<PmxFrameElement>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RigidBodyShape {
    Sphere,
    Box,
    Capsule,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RigidBodyMode {
    StaticWithBone = 0,
    Physics = 1,
    PhysicsWithBone = 2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxRigidBody {
    pub name: PmxText,
    pub bone: Option<u32>,
    pub group: u8,
    pub mask: u16,
    pub shape: RigidBodyShape,
    pub size: Vec3,
    pub position: Vec3,
    pub rotation: Quat,
    pub mass: f32,
    pub damping_translation: f32,
    pub damping_rotation: f32,
    pub restitution: f32,
    pub friction: f32,
    pub mode: RigidBodyMode,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PmxJoint {
    pub name: PmxText,
    pub kind: u8, // 6 = point-to-point+cone-limit (fixed joint)
    pub body_a: Option<u32>,
    pub body_b: Option<u32>,
    pub position: Vec3,
    pub rotation: Quat,
    pub lin_lower: Vec3,
    pub lin_upper: Vec3,
    pub ang_lower: Vec3,
    pub ang_upper: Vec3,
}
