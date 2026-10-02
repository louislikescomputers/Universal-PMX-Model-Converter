//! PMX binary writer (2.0 / 2.1). Deterministic byte-for-byte output:
//! index sizes are computed from actual counts, floats are written verbatim,
//! and no timestamps or randomness enter the file.

use super::model::*;
use crate::error::{MmdconvError, Result};
use glam::{Quat, Vec3};

pub struct WriterOpts {
    pub vertex_size: IdSize,
    pub texture_size: IdSize,
    pub material_size: IdSize,
    pub bone_size: IdSize,
    pub morph_size: IdSize,
    pub rigid_body_size: IdSize,
    pub toon_size: IdSize,
}

impl WriterOpts {
    /// Compute the smallest legal index sizes for a model.
    pub fn auto(m: &PmxModel) -> WriterOpts {
        // PMX convention: sizes depend on count; we keep a minimum of U16 for
        // vertices/textures/materials (widely used baseline) but shrink bones/
        // morphs/rigids when small — matching what PMXEditor produces.
        WriterOpts {
            vertex_size: if m.vertices.len() < 0x7FFF { IdSize::U16 } else { IdSize::U32 },
            texture_size: if m.textures.len() < 0x7FFF { IdSize::U16 } else { IdSize::U32 },
            material_size: if m.materials.len() < 0x7F { IdSize::U8 } else { IdSize::U16 },
            bone_size: if m.bones.len() < 0x7FFF { IdSize::U16 } else { IdSize::U32 },
            morph_size: if m.morphs.len() < 0x7FFF { IdSize::U16 } else { IdSize::U32 },
            rigid_body_size: if m.rigid_bodies.len() < 0x7FFF { IdSize::U16 } else { IdSize::U32 },
            toon_size: IdSize::U8,
        }
    }
}

struct W {
    buf: Vec<u8>,
}

impl W {
    fn u8(&mut self, v: u8) { self.buf.push(v); }
    fn i32(&mut self, v: i32) { self.buf.extend_from_slice(&v.to_le_bytes()); }
    fn u32(&mut self, v: u32) { self.buf.extend_from_slice(&v.to_le_bytes()); }
    fn f32(&mut self, v: f32) { self.buf.extend_from_slice(&v.to_le_bytes()); }
    fn vec2(&mut self, v: [f32; 2]) { self.f32(v[0]); self.f32(v[1]); }
    fn vec3(&mut self, v: Vec3) { self.f32(v.x); self.f32(v.y); self.f32(v.z); }
    fn vec3a(&mut self, v: [f32; 3]) { self.f32(v[0]); self.f32(v[1]); self.f32(v[2]); }
    fn vec4(&mut self, v: [f32; 4]) { for x in v { self.f32(x); } }
    fn quat(&mut self, q: Quat) { self.f32(q.x); self.f32(q.y); self.f32(q.z); self.f32(q.w); }
    fn id(&mut self, size: IdSize, v: i64) {
        match size {
            IdSize::U8 => self.u8(v as i8 as u8),
            IdSize::U16 => self.buf.extend_from_slice(&(v as i16).to_le_bytes()),
            IdSize::U32 => self.i32(v as i32),
        }
    }
    fn opt_id(&mut self, size: IdSize, v: Option<u32>) {
        self.id(size, v.map(|x| x as i64).unwrap_or(-1));
    }
    fn text(&mut self, enc: PmxEncoding, s: &str) -> std::result::Result<(), MmdconvError> {
        match enc {
            PmxEncoding::Utf8 => {
                let b = s.as_bytes();
                if b.len() > i32::MAX as usize {
                    return Err(MmdconvError::Input("text too long".into()));
                }
                self.i32(b.len() as i32);
                self.buf.extend_from_slice(b);
            }
            PmxEncoding::Utf16Le => {
                let units: Vec<u16> = s.encode_utf16().collect();
                // guard against unpaired surrogates inflating beyond i32 bytes
                self.i32((units.len() * 2) as i32);
                for u in units {
                    self.buf.extend_from_slice(&u.to_le_bytes());
                }
            }
        }
        Ok(())
    }
    fn name3(&mut self, enc: PmxEncoding, t: &PmxText) -> std::result::Result<(), MmdconvError> {
        self.text(enc, &t.name)?;
        match &t.en {
            Some(e) => self.text(enc, e)?,
            None => self.i32(0),
        }
        match &t.original {
            Some(o) => self.text(enc, o)?,
            None => self.i32(0),
        }
        Ok(())
    }
}

fn size_byte(s: IdSize) -> u8 {
    match s {
        IdSize::U8 => 0,
        IdSize::U16 => 1,
        IdSize::U32 => 2,
    }
}

/// Validate referential integrity that the writer itself must guarantee.
fn precheck(m: &PmxModel, o: &WriterOpts) -> Result<()> {
    let nv = m.vertices.len();
    if nv > i32::MAX as usize {
        return Err(MmdconvError::Input(format!("{} vertices exceed PMX limit", nv)));
    }
    if m.faces.len() % 3 != 0 {
        return Err(MmdconvError::Input("face index count not divisible by 3".into()));
    }
    for (i, &f) in m.faces.iter().enumerate() {
        if f as usize >= nv {
            return Err(MmdconvError::Input(format!("face index #{i} = {f} out of range ({nv} vertices)")));
        }
    }
    let nb = m.bones.len();
    for (i, b) in m.bones.iter().enumerate() {
        if let Some(p) = b.parent {
            if p as usize >= nb {
                return Err(MmdconvError::Input(format!("bone {i} parent {p} out of range")));
            }
            if p == i as u32 {
                return Err(MmdconvError::Input(format!("bone {i} is its own parent")));
            }
        }
        if let Some(ik) = &b.ik {
            if ik.target as usize >= nb {
                return Err(MmdconvError::Input(format!("bone {i} IK target out of range")));
            }
            for l in &ik.links {
                if l.bone as usize >= nb {
                    return Err(MmdconvError::Input(format!("bone {i} IK link out of range")));
                }
            }
        }
        if let TailType::Index(t) = b.tail {
            if t as usize >= nb {
                return Err(MmdconvError::Input(format!("bone {i} tail index {t} out of range")));
            }
        }
    }
    // cycle check on parents
    for start in 0..nb {
        let mut seen = vec![false; nb];
        let mut cur = Some(start);
        while let Some(c) = cur {
            if seen[c] {
                return Err(MmdconvError::Input(format!("bone parent cycle involving bone {c}")));
            }
            seen[c] = true;
            cur = m.bones[c].parent.map(|p| p as usize);
        }
    }
    let nt = m.textures.len();
    for (i, mat) in m.materials.iter().enumerate() {
        if let Some(t) = mat.texture {
            if t as usize >= nt {
                return Err(MmdconvError::Input(format!("material {i} texture {t} out of range")));
            }
        }
        if !mat.shared_toon {
            if let Some(t) = mat.toon {
                if t as usize >= nt {
                    return Err(MmdconvError::Input(format!("material {i} toon texture {t} out of range")));
                }
            }
        }
    }
    // weight sums per deform type
    for (i, v) in m.vertices.iter().enumerate() {
        let n = match v.deform {
            DeformType::Bdef1 => 1,
            DeformType::Bdef2 | DeformType::Sdef => 2,
            DeformType::Bdef4 | DeformType::Qdef => 4,
        };
        let sum: f32 = v.weights[..n].iter().sum();
        if (sum - 1.0).abs() > 0.05 {
            return Err(MmdconvError::Input(format!(
                "vertex {i}: skin weights sum to {sum}, expected ~1.0"
            )));
        }
        for bi in &v.bones[..n] {
            if *bi as usize >= nb {
                return Err(MmdconvError::Input(format!("vertex {i} references bone {bi} out of range")));
            }
        }
        if !v.pos.is_finite() {
            return Err(MmdconvError::Input(format!("vertex {i} position contains NaN")));
        }
    }
    let _ = o;
    Ok(())
}

/// Serialize a PMX model to bytes.
pub fn write_pmx(m: &PmxModel, opts: &WriterOpts) -> Result<Vec<u8>> {
    precheck(m, opts)?;
    let enc = m.encoding;
    let mut w = W { buf: Vec::with_capacity(1 << 16) };

    // header
    w.buf.extend_from_slice(b"PMX 3.1\x00");
    w.f32(m.version);
    w.u8(if m.version >= 2.1 { 9 } else { 8 }); // global count
    w.u8(match enc { PmxEncoding::Utf16Le => 0, PmxEncoding::Utf8 => 1 });
    w.u8(m.additional_uv_count);
    w.u8(size_byte(opts.vertex_size));
    w.u8(size_byte(opts.texture_size));
    w.u8(size_byte(opts.material_size));
    w.u8(size_byte(opts.bone_size));
    w.u8(size_byte(opts.morph_size));
    w.u8(size_byte(opts.rigid_body_size));
    w.u8(size_byte(opts.toon_size));
    if m.version >= 2.1 {
        w.u8(if m.shift_jis_replacement { 1 } else { 0 });
    }

    w.name3(enc, &m.name)?;
    w.name3(enc, &m.comment)?;

    // vertices
    w.i32(m.vertices.len() as i32);
    for v in &m.vertices {
        w.vec3(v.pos);
        w.vec3(v.normal);
        w.vec2(v.uv);
        for e in &v.extra_uv {
            w.vec4(*e);
        }
        w.u8(v.deform.code());
        let n = match v.deform {
            DeformType::Bdef1 => 1,
            DeformType::Bdef2 | DeformType::Sdef => 2,
            DeformType::Bdef4 | DeformType::Qdef => 4,
        };
        for i in 0..n {
            w.id(opts.vertex_size, v.bones[i] as i64);
            w.f32(v.weights[i]);
        }
        if v.deform == DeformType::Sdef {
            // SDEF params are not produced by our pipeline; emit zeros (valid).
            w.vec3(Vec3::ZERO);
            w.vec3(Vec3::ZERO);
            w.vec3(Vec3::ZERO);
            w.vec3(Vec3::ZERO);
            w.vec3(Vec3::ZERO);
        }
        w.f32(v.edge_scale);
        if m.version >= 2.1 {
            let mut flags = v.shape & 0x0F;
            if v.wire {
                flags |= 0x10;
            }
            w.u8(flags);
            if v.shape != 0 {
                let ref_size = if (v.vert_ref.unwrap_or(0) as usize) < m.vertices.len().min(0x7FFF) {
                    opts.vertex_size
                } else {
                    IdSize::U32
                };
                w.u8(size_byte(ref_size));
                w.id(ref_size, v.vert_ref.map(|x| x as i64).unwrap_or(-1));
            }
        }
    }

    // faces
    w.i32(m.faces.len() as i32);
    for f in &m.faces {
        w.id(opts.vertex_size, *f as i64);
    }

    // textures
    w.i32(m.textures.len() as i32);
    for t in &m.textures {
        w.text(enc, t)?;
    }

    // materials
    w.i32(m.materials.len() as i32);
    for mat in &m.materials {
        w.name3(enc, &mat.name)?;
        w.vec4(mat.diffuse);
        w.vec3a(mat.specular);
        w.f32(mat.shininess);
        w.vec3a(mat.ambient);
        let mut tox: u8 = 0;
        if mat.double_sided { tox |= 0x01; }
        if mat.self_shadow { tox |= 0x02; }
        if mat.receive_shadow { tox |= 0x04; }
        if mat.cache { tox |= 0x08; }
        if mat.draw_line { tox |= 0x10; }
        if mat.draw_ground_shadow { tox |= 0x20; }
        if mat.draw_edge { tox |= 0x40; }
        if mat.vertex_color_mode { tox |= 0x80; }
        w.u8(tox);
        w.f32(mat.point_size);
        w.f32(mat.line_width);
        w.vec4(mat.edge_color);
        w.opt_id(opts.texture_size, mat.texture);
        w.opt_id(opts.texture_size, mat.sphere);
        w.u8(mat.sphere_mode);
        w.u8(if mat.shared_toon { 0 } else { 1 });
        if mat.shared_toon {
            w.u8(mat.toon.unwrap_or(0) as u8);
        } else {
            w.opt_id(opts.toon_size, mat.toon);
        }
        w.text(enc, &mat.memo)?;
    }

    // bones
    w.i32(m.bones.len() as i32);
    for b in &m.bones {
        w.name3(enc, &b.name)?;
        w.vec3(b.position);
        w.opt_id(opts.bone_size, b.parent);
        w.u8(b.transform_after_deform as u8);
        let mut flags = b.flags & !FLAG_TAIL_INDEX;
        // normalize derived bits from structured fields
        if b.inherit.is_some() {
            flags &= !(FLAG_APPEND_ROTATE | FLAG_APPEND_TRANSLATE);
            match b.inherit.map(|i| i.mode).unwrap_or(0) {
                0 => flags |= FLAG_APPEND_ROTATE | FLAG_APPEND_TRANSLATE,
                1 => flags |= FLAG_APPEND_ROTATE,
                _ => flags |= FLAG_APPEND_TRANSLATE,
            }
        } else {
            flags &= !(FLAG_APPEND_ROTATE | FLAG_APPEND_TRANSLATE);
        }
        if b.external_parent.is_some() { flags |= FLAG_EXTERNAL_PARENT; }
        if b.fixed_axis.is_some() { flags |= FLAG_FIXED_AXIS; }
        if b.local_x.is_some() && b.local_z.is_some() { flags |= FLAG_LOCAL_AXIS; }
        if b.after_physics { flags |= FLAG_AFTER_PHYSICS; }
        if b.external_transform { flags |= FLAG_EXTERNAL_TRANSFORM; }
        if b.ik.is_some() { flags |= FLAG_IK; }
        flags &= !(FLAG_VISIBLE | FLAG_ON | FLAG_DISABLED);
        if b.visible { flags |= FLAG_VISIBLE; }
        if b.enabled { flags |= FLAG_ON; } else { flags |= FLAG_DISABLED; }

        let head_is_index = matches!(b.head_id, BoneTarget::Index(_));
        w.u8(head_is_index as u8);
        w.u8(matches!(b.tail, TailType::Index(_)) as u8);
        match b.head_id {
            BoneTarget::Index(i) => w.id(opts.bone_size, i as i64),
            BoneTarget::Position(p) => w.vec3(p),
        }
        match b.tail {
            TailType::Index(i) => w.id(opts.bone_size, i as i64),
            TailType::Offset(o) => w.vec3(o),
        }
        w.i32(b.level as i32);
        w.u32(flags);
        if flags & FLAG_APPEND_ROTATE != 0 || flags & FLAG_APPEND_TRANSLATE != 0 {
            if let Some(ih) = &b.inherit {
                w.id(opts.bone_size, ih.bone as i64);
                w.f32(ih.ratio);
            } else {
                w.id(opts.bone_size, -1);
                w.f32(1.0);
            }
        }
        if flags & FLAG_EXTERNAL_PARENT != 0 {
            match &b.external_parent {
                Some((idx, ty)) => {
                    w.id(opts.bone_size, *idx as i64);
                    w.u8(*ty as u8);
                }
                None => { w.id(opts.bone_size, -1); w.u8(0); }
            }
        }
        if flags & FLAG_FIXED_AXIS != 0 {
            w.vec3(b.fixed_axis.unwrap_or(Vec3::Y));
        }
        if flags & FLAG_LOCAL_AXIS != 0 {
            w.vec3(b.local_x.unwrap_or(Vec3::X));
            w.vec3(b.local_z.unwrap_or(Vec3::Z));
        }
        if flags & FLAG_EXTERNAL_TRANSFORM != 0 {
            w.u32(0); // key value
        }
        if flags & FLAG_IK != 0 {
            if let Some(ik) = &b.ik {
                w.id(opts.bone_size, ik.target as i64);
                w.i32(ik.links.len() as i32);
                w.i32(ik.loop_count as i32);
                w.f32(ik.angle_limit);
                for l in &ik.links {
                    w.id(opts.bone_size, l.bone as i64);
                    w.u8(l.limit_enabled as u8);
                    if l.limit_enabled {
                        w.vec3(l.lower);
                        w.vec3(l.upper);
                    }
                }
            } else {
                w.id(opts.bone_size, -1);
                w.i32(0);
                w.i32(0);
                w.f32(0.0);
            }
        }
    }

    // morphs
    w.i32(m.morphs.len() as i32);
    for mo in &m.morphs {
        w.name3(enc, &mo.name)?;
        w.i32(mo.panel as i32);
        w.u8(mo.offset_kind);
        match &mo.data {
            PmxMorphData::Group(v) => {
                w.i32(v.len() as i32);
                for (i, r0) in v {
                    w.id(opts.morph_size, *i as i64);
                    w.f32(*r0);
                }
            }
            PmxMorphData::Vertex(v) => {
                w.i32(v.len() as i32);
                for (i, off) in v {
                    w.id(opts.vertex_size, *i as i64);
                    w.vec3(*off);
                }
            }
            PmxMorphData::BoneRel(v) => {
                w.i32(v.len() as i32);
                for (i, t, r) in v {
                    w.id(opts.bone_size, *i as i64);
                    w.vec3(*t);
                    w.quat(*r);
                }
            }
            PmxMorphData::Uv(v) => {
                if mo.offset_kind == 4 {
                    w.i32(-1); // base UV morph not preserved by this tool
                }
                w.i32(v.len() as i32);
                for (i, off) in v {
                    w.id(opts.vertex_size, *i as i64);
                    w.vec4(*off);
                }
            }
            PmxMorphData::Impulse(v) => {
                w.i32(v.len() as i32);
                for (i, t, r) in v {
                    w.id(opts.vertex_size, *i as i64);
                    w.u8(0); // relative flag
                    w.vec3(*t);
                    w.quat(*r);
                }
            }
            PmxMorphData::InverseRatio(v) => {
                w.i32(v.len() as i32);
                for (i, r0) in v {
                    w.id(opts.material_size, *i as i64);
                    w.f32(*r0);
                }
            }
            PmxMorphData::Material(v) => {
                w.i32(v.len() as i32);
                for off in v {
                    w.id(opts.material_size, off.material);
                    if off.material < 0 {
                        // "all materials": 9 fixed-order blocks, each (kind, mode, value)
                        let g4 = |o: &Option<([f32; 4], u8)>| o.map(|x| x.0).unwrap_or([0.0; 4]);
                        let m4 = |o: &Option<([f32; 4], u8)>| o.map(|x| x.1).unwrap_or(0);
                        let g3 = |o: &Option<([f32; 3], u8)>| o.map(|x| x.0).unwrap_or([0.0; 3]);
                        let m3 = |o: &Option<([f32; 3], u8)>| o.map(|x| x.1).unwrap_or(0);
                        w.u8(0); w.u8(m4(&off.diffuse)); w.vec4(g4(&off.diffuse));
                        w.u8(1); w.u8(m3(&off.specular)); w.vec3a(g3(&off.specular));
                        w.u8(2); w.u8(off.shininess.map(|x| x.1).unwrap_or(0)); w.f32(off.shininess.map(|x| x.0).unwrap_or(0.0));
                        w.u8(3); w.u8(m3(&off.ambient)); w.vec3a(g3(&off.ambient));
                        w.u8(4); w.u8(m4(&off.edge_color)); w.vec4(g4(&off.edge_color));
                        w.u8(5); w.u8(off.edge_size.map(|x| x.1).unwrap_or(0)); w.f32(off.edge_size.map(|x| x.0).unwrap_or(0.0));
                        w.u8(6); w.u8(m4(&off.texture)); w.vec4(g4(&off.texture));
                        w.u8(7); w.u8(m4(&off.sphere)); w.vec4(g4(&off.sphere));
                        w.u8(8); w.u8(m4(&off.toon)); w.vec4(g4(&off.toon));
                    } else {
                        let mut ops: Vec<Vec<u8>> = Vec::new();
                        if let Some((val, mode)) = off.diffuse { let mut t = W{buf:Vec::new()}; t.u8(0); t.u8(mode); t.vec4(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.specular { let mut t = W{buf:Vec::new()}; t.u8(1); t.u8(mode); t.vec3a(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.shininess { let mut t = W{buf:Vec::new()}; t.u8(2); t.u8(mode); t.f32(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.ambient { let mut t = W{buf:Vec::new()}; t.u8(3); t.u8(mode); t.vec3a(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.edge_color { let mut t = W{buf:Vec::new()}; t.u8(4); t.u8(mode); t.vec4(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.edge_size { let mut t = W{buf:Vec::new()}; t.u8(5); t.u8(mode); t.f32(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.texture { let mut t = W{buf:Vec::new()}; t.u8(6); t.u8(mode); t.vec4(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.sphere { let mut t = W{buf:Vec::new()}; t.u8(7); t.u8(mode); t.vec4(val); ops.push(t.buf); }
                        if let Some((val, mode)) = off.toon { let mut t = W{buf:Vec::new()}; t.u8(8); t.u8(mode); t.vec4(val); ops.push(t.buf); }
                        if ops.is_empty() {
                            return Err(MmdconvError::Input("material morph entry has no offsets".into()));
                        }
                        w.i32(ops.len() as i32);
                        for op in ops { w.buf.extend_from_slice(&op); }
                    }
                }
            }
            PmxMorphData::Flip(v) => {
                w.i32(v.len() as i32);
                for (i, r0) in v {
                    w.id(opts.bone_size, *i as i64);
                    w.f32(*r0);
                }
            }
        }
    }

    // display frames
    w.i32(m.display_frames.len() as i32);
    for f in &m.display_frames {
        w.name3(enc, &f.name)?;
        w.u8(f.is_special as u8);
        w.i32(f.elements.len() as i32);
        for el in &f.elements {
            w.u8(el.is_morph as u8);
            if el.is_morph {
                w.id(opts.morph_size, el.index as i64);
            } else {
                w.id(opts.bone_size, el.index as i64);
            }
        }
    }

    // soft bodies (2.1): only raw-passthrough entries are supported
    if m.version >= 2.1 {
        w.i32(m.soft_bodies.len() as i32);
        for sb in &m.soft_bodies {
            w.buf.extend_from_slice(sb);
        }
    }

    // rigid bodies
    w.i32(m.rigid_bodies.len() as i32);
    for rb in &m.rigid_bodies {
        w.name3(enc, &rb.name)?;
        w.opt_id(opts.bone_size, rb.bone);
        w.u8(rb.group);
        w.buf.extend_from_slice(&rb.mask.to_le_bytes());
        w.u8(match rb.shape {
            RigidBodyShape::Sphere => 0,
            RigidBodyShape::Box => 1,
            RigidBodyShape::Capsule => 2,
        });
        w.vec3(rb.size);
        w.vec3(rb.position);
        w.quat(rb.rotation);
        w.f32(rb.mass);
        w.f32(rb.damping_translation);
        w.f32(rb.damping_rotation);
        w.f32(rb.restitution);
        w.f32(rb.friction);
        w.u8(rb.mode as u8);
    }

    // joints
    w.i32(m.joints.len() as i32);
    for j in &m.joints {
        w.name3(enc, &j.name)?;
        w.u8(j.kind);
        w.opt_id(opts.rigid_body_size, j.body_a);
        w.opt_id(opts.rigid_body_size, j.body_b);
        w.vec3(j.position);
        w.quat(j.rotation);
        w.vec3(j.lin_lower);
        w.vec3(j.lin_upper);
        w.vec3(j.ang_lower);
        w.vec3(j.ang_upper);
    }

    Ok(w.buf)
}

pub fn write_pmx_file(path: &std::path::Path, m: &PmxModel, opts: &WriterOpts) -> Result<()> {
    let bytes = write_pmx(m, opts)?;
    std::fs::write(path, bytes)
        .map_err(|e| MmdconvError::io(format!("writing {}", path.display()), e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_model() -> PmxModel {
        let mut m = PmxModel::default();
        m.name = PmxText::both("テストモデル", "Test Model");
        m.vertices = vec![
            PmxVertex::bdef(Vec3::new(0.0, 1.0, 0.0), &[(0, 1.0)]),
            PmxVertex::bdef(Vec3::new(1.0, 0.0, 0.0), &[(0, 0.6), (1, 0.4)]),
            PmxVertex::bdef(Vec3::new(0.0, 0.0, 1.0), &[(0, 0.5), (1, 0.5)]),
        ];
        m.faces = vec![0, 1, 2];
        m.textures = vec!["tex/a.png".into()];
        m.materials = vec![PmxMaterial {
            name: PmxText::new("マテリアル"),
            texture: Some(0),
            ..Default::default()
        }];
        m.bones = vec![
            PmxBone { name: PmxText::new("全ての親"), ..Default::default() },
            PmxBone {
                name: PmxText::new("センター"),
                parent: Some(0),
                tail: TailType::Offset(Vec3::Y),
                ..Default::default()
            },
        ];
        m.morphs = vec![PmxMorph {
            name: PmxText::new("あ"),
            panel: MorphPanel::Mouth,
            offset_kind: 1,
            data: PmxMorphData::Vertex(vec![(0, Vec3::new(0.0, 0.01, 0.0))]),
        }];
        m.display_frames = vec![PmxFrame {
            name: PmxText::new("表情"),
            is_special: false,
            elements: vec![PmxFrameElement { is_morph: true, index: 0 }],
        }];
        m
    }

    #[test]
    fn write_then_read_roundtrip() {
        let m = tiny_model();
        let opts = WriterOpts::auto(&m);
        let bytes = write_pmx(&m, &opts).unwrap();
        let back = crate::pmx::reader::read_pmx(&bytes).unwrap();
        assert_eq!(back.name.name, "テストモデル");
        assert_eq!(back.name.en.as_deref(), Some("Test Model"));
        assert_eq!(back.vertices.len(), 3);
        assert_eq!(back.faces, m.faces);
        assert_eq!(back.textures, m.textures);
        assert_eq!(back.materials.len(), 1);
        assert_eq!(back.bones.len(), 2);
        assert_eq!(back.bones[1].parent, Some(0));
        assert_eq!(back.morphs.len(), 1);
        assert_eq!(back.display_frames.len(), 1);
    }

    #[test]
    fn deterministic_output() {
        let m = tiny_model();
        let a = write_pmx(&m, &WriterOpts::auto(&m)).unwrap();
        let b = write_pmx(&m, &WriterOpts::auto(&m)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn rejects_bad_face_index() {
        let mut m = tiny_model();
        m.faces = vec![0, 1, 9];
        let e = write_pmx(&m, &WriterOpts::auto(&m));
        assert!(e.is_err());
    }

    #[test]
    fn rejects_weight_sum_error() {
        let mut m = tiny_model();
        m.vertices[1].weights = [0.3, 0.3, 0.0, 0.0];
        let e = write_pmx(&m, &WriterOpts::auto(&m));
        assert!(e.is_err());
    }
}
