//! PMX binary reader (2.0 / 2.1). Never panics on malformed input: every read
//! is bounds-checked and returns a structured error with the byte offset.

use super::model::*;
use crate::error::{MmdconvError, Result};
use glam::{Quat, Vec3};

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
    encoding: PmxEncoding,
    vs: IdSize,
    ts: IdSize,
    ms: IdSize,
    bs: IdSize,
    mos: IdSize,
    rbs: IdSize,
    tos: IdSize,
    add_uv: u8,
}

type R<T> = std::result::Result<T, MmdconvError>;

impl<'a> Reader<'a> {
    fn need(&self, n: usize) -> R<()> {
        if self.p + n > self.d.len() {
            return Err(MmdconvError::pmx_parse(
                self.p,
                format!("unexpected end of file (wanted {n} bytes, {} left)", self.d.len() - self.p),
            ));
        }
        Ok(())
    }
    fn u8(&mut self) -> R<u8> {
        self.need(1)?;
        let v = self.d[self.p];
        self.p += 1;
        Ok(v)
    }
    fn i32(&mut self) -> R<i32> {
        self.need(4)?;
        let v = i32::from_le_bytes([self.d[self.p], self.d[self.p + 1], self.d[self.p + 2], self.d[self.p + 3]]);
        self.p += 4;
        Ok(v)
    }
    fn u32(&mut self) -> R<u32> {
        Ok(self.i32()? as u32)
    }
    fn f32(&mut self) -> R<f32> {
        self.need(4)?;
        let v = f32::from_le_bytes([self.d[self.p], self.d[self.p + 1], self.d[self.p + 2], self.d[self.p + 3]]);
        self.p += 4;
        Ok(v)
    }
    fn vec2(&mut self) -> R<[f32; 2]> {
        Ok([self.f32()?, self.f32()?])
    }
    fn vec3(&mut self) -> R<Vec3> {
        Ok(Vec3::new(self.f32()?, self.f32()?, self.f32()?))
    }
    fn vec4(&mut self) -> R<[f32; 4]> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
    fn quat(&mut self) -> R<Quat> {
        // stored x,y,z,w
        let x = self.f32()?;
        let y = self.f32()?;
        let z = self.f32()?;
        let w = self.f32()?;
        Ok(Quat::from_xyzw(x, y, z, w))
    }
    fn id_of(&mut self, size: IdSize) -> R<i64> {
        let v = match size {
            IdSize::U8 => self.u8()? as i8 as i64,
            IdSize::U16 => {
                self.need(2)?;
                let v = u16::from_le_bytes([self.d[self.p], self.d[self.p + 1]]) as i16 as i64;
                self.p += 2;
                v
            }
            IdSize::U32 => self.i32()? as i64,
        };
        Ok(v)
    }
    fn vertex_id(&mut self) -> R<i64> { self.id_of(self.vs) }
    fn texture_id(&mut self) -> R<i64> { self.id_of(self.ts) }
    fn material_id(&mut self) -> R<i64> { self.id_of(self.ms) }
    fn bone_id(&mut self) -> R<i64> { self.id_of(self.bs) }
    fn morph_id(&mut self) -> R<i64> { self.id_of(self.mos) }
    fn rigid_id(&mut self) -> R<i64> { self.id_of(self.rbs) }

    /// Read an index that must be non-negative and < limit (-1 → None).
    fn idx(&mut self, size: IdSize, limit: usize, what: &str) -> R<Option<u32>> {
        let v = self.id_of(size)?;
        if v < 0 {
            Ok(None)
        } else if (v as usize) >= limit {
            Err(MmdconvError::pmx_parse(self.p, format!("{what} index {v} out of range (limit {limit})")))
        } else {
            Ok(Some(v as u32))
        }
    }

    fn text(&mut self) -> R<String> {
        let len = self.i32()?;
        if len < 0 {
            return Err(MmdconvError::pmx_parse(self.p - 4, "negative text length"));
        }
        let len = len as usize;
        self.need(len)?;
        let bytes = &self.d[self.p..self.p + len];
        self.p += len;
        match self.encoding {
            PmxEncoding::Utf8 => Ok(String::from_utf8_lossy(bytes).into_owned()),
            PmxEncoding::Utf16Le => {
                if len % 2 != 0 {
                    return Err(MmdconvError::pmx_parse(self.p, "UTF-16 text with odd byte length"));
                }
                let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                Ok(String::from_utf16_lossy(&units))
            }
        }
    }

    fn name3(&mut self) -> R<PmxText> {
        let name = self.text()?;
        let en_len = self.i32()?;
        let en = if en_len > 0 { Some(self.text()?) } else { None };
        let org_len = self.i32()?;
        let original = if org_len > 0 { Some(self.text()?) } else { None };
        Ok(PmxText { name, en, original })
    }
}

fn size_from_byte(b: u8) -> R<IdSize> {
    match b {
        0 => Ok(IdSize::U8),
        1 => Ok(IdSize::U16),
        2 => Ok(IdSize::U32),
        other => Err(MmdconvError::pmx_parse(8, format!("invalid index size byte {other}"))),
    }
}

/// Parse a complete PMX buffer.
pub fn read_pmx(data: &[u8]) -> Result<PmxModel> {
    if data.len() < 9 {
        return Err(MmdconvError::pmx_parse(0, "file too short to be PMX"));
    }
    if &data[0..8] != b"PMX 3.1\0" {
        return Err(MmdconvError::pmx_parse(0, "missing PMX magic header"));
    }
    let mut r = Reader {
        d: data,
        p: 4,
        encoding: PmxEncoding::Utf16Le,
        vs: IdSize::U32,
        ts: IdSize::U32,
        ms: IdSize::U32,
        bs: IdSize::U32,
        mos: IdSize::U32,
        rbs: IdSize::U32,
        tos: IdSize::U32,
        add_uv: 0,
    };
    let ver = r.f32()?;
    if (ver - 2.0).abs() > 1e-6 && (ver - 2.1).abs() > 1e-6 {
        return Err(MmdconvError::pmx_parse(4, format!("unsupported PMX version {ver}")));
    }
    let global_count = r.u8()?;
    if global_count < 8 {
        return Err(MmdconvError::pmx_parse(r.p - 1, format!("global count {global_count} < 8")));
    }
    r.encoding = match r.u8()? {
        0 => PmxEncoding::Utf16Le,
        1 => PmxEncoding::Utf8,
        other => return Err(MmdconvError::pmx_parse(r.p - 1, format!("invalid encoding {other}"))),
    };
    r.add_uv = r.u8()?;
    if r.add_uv > 4 {
        return Err(MmdconvError::pmx_parse(r.p - 1, format!("additional UV count {} > 4", r.add_uv)));
    }
    r.vs = size_from_byte(r.u8()?)?;
    r.ts = size_from_byte(r.u8()?)?;
    r.ms = size_from_byte(r.u8()?)?;
    r.bs = size_from_byte(r.u8()?)?;
    r.mos = size_from_byte(r.u8()?)?;
    r.rbs = size_from_byte(r.u8()?)?;
    r.tos = size_from_byte(r.u8()?)?;
    for _ in 8..global_count {
        let _ = r.u8()?;
    }

    let mut m = PmxModel {
        version: ver,
        encoding: r.encoding,
        additional_uv_count: r.add_uv,
        vertex_size: r.vs,
        texture_size: r.ts,
        material_size: r.ms,
        bone_size: r.bs,
        morph_size: r.mos,
        rigid_body_size: r.rbs,
        toon_size: r.tos,
        ..Default::default()
    };
    m.name = r.name3()?;
    m.comment = r.name3()?;

    // ---- vertices ----
    let nv = r.i32()?;
    check_count(nv, "vertices", data.len())?;
    for _ in 0..nv {
        let pos = r.vec3()?;
        let normal = r.vec3()?;
        let uv = r.vec2()?;
        let mut extra_uv = Vec::with_capacity(r.add_uv as usize);
        for _ in 0..r.add_uv {
            extra_uv.push(r.vec4()?);
        }
        let dt = DeformType::from_u8(r.u8()?)
            .ok_or_else(|| MmdconvError::pmx_parse(r.p - 1, "invalid deform type"))?;
        let mut bones = [0u32; 4];
        let mut weights = [0f32; 4];
        let nb = match dt {
            DeformType::Bdef1 => 1,
            DeformType::Bdef2 | DeformType::Sdef => 2,
            DeformType::Bdef4 | DeformType::Qdef => 4,
        };
        for i in 0..nb {
            // bone indices are validated once the bone section is parsed
            let b = r.bone_id()?;
            bones[i] = if b < 0 { u32::MAX } else { b as u32 };
            weights[i] = r.f32()?;
        }
        if dt == DeformType::Sdef {
            let _cs = r.vec3()?;
            let _c0 = r.vec3()?;
            let _c1 = r.vec3()?;
            let _r0 = r.vec3()?;
            let _r1 = r.vec3()?;
        }
        let edge_scale = r.f32()?;
        let mut shape = 0u8;
        let mut wire = false;
        let mut vert_ref = None;
        if (ver - 2.1).abs() < 1e-6 {
            let flags = r.u8()?;
            shape = flags & 0x0F;
            wire = (flags >> 4) & 1 == 1;
            if shape != 0 {
                let ref_sz = size_from_byte(r.u8()?)?;
                let rid = r.id_of(ref_sz)?;
                vert_ref = if rid >= 0 { Some(rid as u32) } else { None };
            }
        }
        m.vertices.push(PmxVertex {
            pos, normal, uv, extra_uv, deform: dt, bones, weights,
            edge_scale, shape, wire, vert_ref,
        });
    }

    // ---- faces ----
    let nf = r.i32()?;
    check_count(nf, "faces", data.len())?;
    if nf % 3 != 0 {
        return Err(MmdconvError::pmx_parse(r.p, format!("face index count {nf} is not a multiple of 3")));
    }
    m.faces.reserve(nf as usize);
    for _ in 0..nf {
        let v = r.vertex_id()?;
        if v < 0 || v >= nv as i64 {
            return Err(MmdconvError::pmx_parse(r.p, format!("face vertex index {v} out of range (0..{nv})")));
        }
        m.faces.push(v as u32);
    }

    // ---- textures ----
    let nt = r.i32()?;
    check_count(nt, "textures", data.len())?;
    for _ in 0..nt {
        m.textures.push(r.text()?);
    }

    // ---- materials ----
    let nm = r.i32()?;
    check_count(nm, "materials", data.len())?;
    for _ in 0..nm {
        let name = r.name3()?;
        let diffuse = r.vec4()?;
        let specular = r.vec3()?;
        let shininess = r.f32()?;
        let ambient = r.vec3()?;
        let tox = r.u8()?;
        let mut mat = PmxMaterial {
            name,
            diffuse,
            specular: specular.to_array(),
            shininess,
            ambient: ambient.to_array(),
            double_sided: tox & 0x01 != 0,
            self_shadow: tox & 0x02 != 0,
            receive_shadow: tox & 0x04 != 0,
            cache: tox & 0x08 != 0,
            draw_line: tox & 0x10 != 0,
            draw_ground_shadow: tox & 0x20 != 0,
            draw_edge: tox & 0x40 != 0,
            vertex_color_mode: tox & 0x80 != 0,
            ..Default::default()
        };
        mat.point_size = r.f32()?;
        mat.line_width = r.f32()?;
        mat.edge_color = r.vec4()?;
        mat.texture = r.idx(r.ts, nt as usize, "texture")?;
        mat.sphere = r.idx(r.ts, nt as usize, "sphere")?;
        mat.sphere_mode = r.u8()?;
        mat.shared_toon = r.u8()? == 0;
        if mat.shared_toon {
            let t = r.u8()?;
            mat.toon = Some(t as u32);
        } else {
            mat.toon = r.idx(r.tos, nt as usize, "toon texture")?;
        }
        mat.memo = r.text()?;
        m.materials.push(mat);
    }

    // ---- bones ----
    let nbones = r.i32()?;
    check_count(nbones, "bones", data.len())?;
    for _ in 0..nbones {
        let name = r.name3()?;
        let position = r.vec3()?;
        let parent = r.idx(r.bs, nbones as usize, "bone parent")?;
        let transform_after_deform = r.u8()? != 0;
        let head_is_index = r.u8()? != 0;
        let tail_is_index = r.u8()? != 0;
        let head_id = if head_is_index {
            BoneTarget::Index(r.idx(r.bs, nbones as usize, "bone head")?.unwrap_or(0))
        } else {
            BoneTarget::Position(r.vec3()?)
        };
        let tail = if tail_is_index {
            TailType::Index(r.idx(r.bs, nbones as usize, "bone tail")?.unwrap_or(0))
        } else {
            TailType::Offset(r.vec3()?)
        };
        let level = r.i32()?;
        let flags = r.u32()?;
        let mut bone = PmxBone {
            name,
            position,
            parent,
            transform_after_deform,
            head_id,
            tail,
            level: level as u32,
            flags,
            visible: flags & FLAG_VISIBLE != 0,
            enabled: flags & FLAG_ON != 0,
            ..Default::default()
        };
        if flags & FLAG_APPEND_ROTATE != 0 || flags & FLAG_APPEND_TRANSLATE != 0 {
            let b = r.idx(r.bs, nbones as usize, "append parent")?;
            let ratio = r.f32()?;
            let mode = if flags & FLAG_APPEND_ROTATE != 0 && flags & FLAG_APPEND_TRANSLATE != 0 {
                0
            } else if flags & FLAG_APPEND_ROTATE != 0 {
                1
            } else {
                2
            };
            bone.inherit = Some(Inherit { bone: b.unwrap_or(0), ratio, mode });
        }
        if flags & FLAG_EXTERNAL_PARENT != 0 {
            let b = r.idx(r.bs, nbones as usize, "external parent")?;
            let t = r.u8()?;
            bone.external_parent = Some((b.unwrap_or(0), t as u32));
        }
        if flags & FLAG_FIXED_AXIS != 0 {
            bone.fixed_axis = Some(r.vec3()?);
        }
        if flags & FLAG_LOCAL_AXIS != 0 {
            bone.local_x = Some(r.vec3()?);
            bone.local_z = Some(r.vec3()?);
        }
        if flags & FLAG_AFTER_PHYSICS != 0 {
            bone.after_physics = true;
        }
        if flags & FLAG_EXTERNAL_TRANSFORM != 0 {
            bone.external_transform = true;
            let _key = r.u32()?;
        }
        if flags & FLAG_IK != 0 {
            let target = r.idx(r.bs, nbones as usize, "IK target")?.unwrap_or(u32::MAX);
            let link_count = r.i32()?;
            check_count(link_count, "IK links", data.len())?;
            let loop_count = r.i32()?;
            let angle_limit = r.f32()?;
            let mut links = Vec::with_capacity(link_count.max(0) as usize);
            for _ in 0..link_count {
                let lb = r.idx(r.bs, nbones as usize, "IK link")?.unwrap_or(u32::MAX);
                let has_limit = r.u8()? != 0;
                let (lower, upper) = if has_limit { (r.vec3()?, r.vec3()?) } else { (Vec3::ZERO, Vec3::ZERO) };
                links.push(PmxIkLink { bone: lb, limit_enabled: has_limit, lower, upper });
            }
            bone.ik = Some(PmxIk { target, loop_count: loop_count as u32, angle_limit, links });
        }
        m.bones.push(bone);
    }

    // validate deferred skin-bone references now that we know the bone count
    for (vi, v) in m.vertices.iter().enumerate() {
        let n = match v.deform {
            DeformType::Bdef1 => 1,
            DeformType::Bdef2 | DeformType::Sdef => 2,
            DeformType::Bdef4 | DeformType::Qdef => 4,
        };
        for bi in &v.bones[..n] {
            if *bi == u32::MAX || (*bi as usize) >= nbones as usize {
                return Err(MmdconvError::pmx_parse(
                    0,
                    format!("vertex {vi} references invalid bone index (bones: {nbones})"),
                ));
            }
        }
    }

    // ---- morphs ----
    let nmo = r.i32()?;
    check_count(nmo, "morphs", data.len())?;
    for mi in 0..nmo {
        let name = r.name3()?;
        let panel_raw = r.i32()?;
        let panel = match panel_raw {
            0 => MorphPanel::Other,
            1 => MorphPanel::Category,
            2 => MorphPanel::Brow,
            3 => MorphPanel::Mouth,
            4 => MorphPanel::Eye,
            5 => MorphPanel::Lip,
            _ => MorphPanel::Other,
        };
        let offset_kind = r.u8()?;
        let n_off = r.i32()?;
        check_count(n_off, "morph offsets", data.len())?;
        let data_enum = match offset_kind {
            0 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.idx(r.mos, nmo as usize, "group morph")?.unwrap_or(0);
                    let w = r.f32()?;
                    v.push((i, w));
                }
                PmxMorphData::Group(v)
            }
            1 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.vertex_id()?;
                    if i < 0 || i >= nv as i64 {
                        return Err(MmdconvError::pmx_parse(r.p, format!("vertex morph index {i} out of range")));
                    }
                    v.push((i as u32, r.vec3()?));
                }
                PmxMorphData::Vertex(v)
            }
            2 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.bone_id()?;
                    if i < 0 || i >= nbones as i64 {
                        return Err(MmdconvError::pmx_parse(r.p, format!("bone morph index {i} out of range")));
                    }
                    v.push((i as u32, r.vec3()?, r.quat()?));
                }
                PmxMorphData::BoneRel(v)
            }
            3 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.vertex_id()?;
                    if i < 0 || i >= nv as i64 {
                        return Err(MmdconvError::pmx_parse(r.p, "UV morph index out of range"));
                    }
                    v.push((i as u32, r.vec4()?));
                }
                PmxMorphData::Uv(v)
            }
            4 => {
                // extra UV morph: base-morph reference consumed, entries stored flat
                let _base_morph = r.i32()?;
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.vertex_id()?;
                    if i < 0 || i >= nv as i64 {
                        return Err(MmdconvError::pmx_parse(r.p, "extra-UV morph index out of range"));
                    }
                    v.push((i as u32, r.vec4()?));
                }
                PmxMorphData::Uv(v)
            }
            5 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.vertex_id()?;
                    if i < 0 || i >= nv as i64 {
                        return Err(MmdconvError::pmx_parse(r.p, "impulse morph index out of range"));
                    }
                    let _local_flag = r.u8()?;
                    v.push((i as u32, r.vec3()?, r.quat()?));
                }
                PmxMorphData::Impulse(v)
            }
            6 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.idx(r.bs, nbones as usize, "inverse-ratio morph")?.unwrap_or(0);
                    let off = r.f32()?;
                    v.push((i, off));
                }
                PmxMorphData::InverseRatio(v)
            }
            7 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let mat = r.material_id()?;
                    if mat < -1 || mat >= nm as i64 {
                        return Err(MmdconvError::pmx_parse(r.p, format!("material morph index {mat} out of range")));
                    }
                    let mut o = PmxMatMorphOff {
                        material: mat,
                        diffuse: None, specular: None, shininess: None, ambient: None,
                        edge_color: None, edge_size: None, texture: None, sphere: None, toon: None,
                    };
                    if mat == -1 {
                        // "all materials": 9 fixed-order blocks, each (kind, mode, value)
                        for kind in 0..9u8 {
                            let k = r.u8()?;
                            if k != kind {
                                return Err(MmdconvError::pmx_parse(r.p - 1, format!("material morph (all): expected kind {kind}, got {k}")));
                            }
                            let mode = r.u8()?;
                            match kind {
                                0 => o.diffuse = Some((r.vec4()?, mode)),
                                1 => o.specular = Some((r.vec3()?.to_array(), mode)),
                                2 => o.shininess = Some((r.f32()?, mode)),
                                3 => o.ambient = Some((r.vec3()?.to_array(), mode)),
                                4 => o.edge_color = Some((r.vec4()?, mode)),
                                5 => o.edge_size = Some((r.f32()?, mode)),
                                6 => o.texture = Some((r.vec4()?, mode)),
                                7 => o.sphere = Some((r.vec4()?, mode)),
                                _ => o.toon = Some((r.vec4()?, mode)),
                            }
                        }
                    } else {
                        let count = r.i32()?;
                        check_count(count, "material morph ops", data.len())?;
                        for _ in 0..count {
                            let kind = r.u8()?;
                            let mode = r.u8()?;
                            match kind {
                                0 => o.diffuse = Some((r.vec4()?, mode)),
                                1 => o.specular = Some((r.vec3()?.to_array(), mode)),
                                2 => o.shininess = Some((r.f32()?, mode)),
                                3 => o.ambient = Some((r.vec3()?.to_array(), mode)),
                                4 => o.edge_color = Some((r.vec4()?, mode)),
                                5 => o.edge_size = Some((r.f32()?, mode)),
                                6 => o.texture = Some((r.vec4()?, mode)),
                                7 => o.sphere = Some((r.vec4()?, mode)),
                                8 => o.toon = Some((r.vec4()?, mode)),
                                other => return Err(MmdconvError::pmx_parse(r.p - 1, format!("bad material morph kind {other}"))),
                            }
                        }
                    }
                    v.push(o);
                }
                PmxMorphData::Material(v)
            }
            8 => {
                let mut v = Vec::with_capacity(n_off as usize);
                for _ in 0..n_off {
                    let i = r.idx(r.bs, nbones as usize, "flip morph")?.unwrap_or(0);
                    let r0 = r.f32()?;
                    v.push((i, r0));
                }
                PmxMorphData::Flip(v)
            }
            other => return Err(MmdconvError::pmx_parse(r.p - 1, format!("unknown morph offset kind {other}"))),
        };
        let _ = mi;
        m.morphs.push(PmxMorph { name, panel, offset_kind, data: data_enum });
    }

    // ---- display frames ----
    let ndf = r.i32()?;
    check_count(ndf, "display frames", data.len())?;
    for _ in 0..ndf {
        let name = r.name3()?;
        let is_special = r.u8()? != 0;
        let nel = r.i32()?;
        check_count(nel, "frame elements", data.len())?;
        let mut elements = Vec::with_capacity(nel.max(0) as usize);
        for _ in 0..nel {
            let el_type = r.u8()?;
            let idx = if el_type == 1 {
                r.idx(r.mos, nmo as usize, "frame morph element")?.unwrap_or(0)
            } else {
                r.idx(r.bs, nbones as usize, "frame bone element")?.unwrap_or(0)
            };
            elements.push(PmxFrameElement { is_morph: el_type == 1, index: idx });
        }
        m.display_frames.push(PmxFrame { name, is_special, elements });
    }

    // ---- soft bodies (2.1): preserved raw ----
    if (ver - 2.1).abs() < 1e-6 {
        let nsb = r.i32()?;
        check_count(nsb, "soft bodies", data.len())?;
        for _ in 0..nsb {
            let start = r.p;
            skip_soft_body(&mut r, nbones, nv, nm, data.len())?;
            m.soft_bodies.push(data[start..r.p].to_vec());
        }
    }

    // ---- rigid bodies ----
    let nrb = r.i32()?;
    check_count(nrb, "rigid bodies", data.len())?;
    for _ in 0..nrb {
        let name = r.name3()?;
        let bone = r.idx(r.bs, nbones as usize, "rigid body bone")?;
        let group = r.u8()?;
        r.need(2)?;
        let mask = u16::from_le_bytes([r.d[r.p], r.d[r.p + 1]]);
        r.p += 2;
        let shape = match r.u8()? {
            0 => RigidBodyShape::Sphere,
            1 => RigidBodyShape::Box,
            2 => RigidBodyShape::Capsule,
            other => return Err(MmdconvError::pmx_parse(r.p - 1, format!("bad rigid shape {other}"))),
        };
        let size = r.vec3()?;
        let position = r.vec3()?;
        let rotation = r.quat()?;
        let mass = r.f32()?;
        let damping_translation = r.f32()?;
        let damping_rotation = r.f32()?;
        let restitution = r.f32()?;
        let friction = r.f32()?;
        let mode = match r.u8()? {
            0 => RigidBodyMode::StaticWithBone,
            1 => RigidBodyMode::Physics,
            2 => RigidBodyMode::PhysicsWithBone,
            other => return Err(MmdconvError::pmx_parse(r.p - 1, format!("bad rigid mode {other}"))),
        };
        m.rigid_bodies.push(PmxRigidBody {
            name, bone, group, mask, shape, size, position, rotation,
            mass, damping_translation, damping_rotation, restitution, friction, mode,
        });
    }

    // ---- joints ----
    let nj = r.i32()?;
    check_count(nj, "joints", data.len())?;
    for _ in 0..nj {
        let name = r.name3()?;
        let kind = r.u8()?;
        let a = r.rigid_id()?;
        let b = r.rigid_id()?;
        if a >= 0 && a >= nrb as i64 {
            return Err(MmdconvError::pmx_parse(r.p, format!("joint body A index {a} out of range")));
        }
        if b >= 0 && b >= nrb as i64 {
            return Err(MmdconvError::pmx_parse(r.p, format!("joint body B index {b} out of range")));
        }
        let position = r.vec3()?;
        let rotation = r.quat()?;
        let lin_lower = r.vec3()?;
        let lin_upper = r.vec3()?;
        let ang_lower = r.vec3()?;
        let ang_upper = r.vec3()?;
        m.joints.push(PmxJoint {
            name, kind,
            body_a: if a < 0 { None } else { Some(a as u32) },
            body_b: if b < 0 { None } else { Some(b as u32) },
            position, rotation, lin_lower, lin_upper, ang_lower, ang_upper,
        });
    }

    if r.p != data.len() {
        m.comment.original = Some(format!(
            "trailing {} bytes ignored after joint section",
            data.len() - r.p
        ));
    }
    Ok(m)
}

/// PMX 2.1 soft body section — parsed structurally so we know its extent; the
/// raw bytes are preserved for re-export.
fn skip_soft_body(r: &mut Reader, nbones: i32, nverts: i32, nmat: i32, file_len: usize) -> Result<()> {
    let _name = r.name3()?;
    let _comment = r.text()?;
    let _shape = r.u8()?;
    let _material = r.idx(r.ms, nmat.max(0) as usize, "softbody material")?;
    let _flags = r.u8()?;
    let _margin = r.f32()?;
    let _ao = r.u8()?;
    let _target = r.i32()?;
    let _guard = r.i32()?;
    let _local = r.i32()?;
    let _wb = r.u8()?;
    let _boud = r.i32()?;
    let _cluster = r.u8()?;
    for _ in 0..14 {
        let _ = r.f32()?;
    }
    // config cluster
    let _cc = r.u8()?;
    let _cf = r.u32()?;
    for _ in 0..11 {
        let _ = r.f32()?;
    }
    let _cisrf = r.u8()?;
    let nn = r.i32()?;
    check_count(nn, "softbody nodes", file_len)?;
    for _ in 0..nn {
        let _vi = r.idx(r.vs, nverts.max(0) as usize, "softbody node vertex")?;
        let _m = r.f32()?;
        let _tv = r.vec3()?;
        let _rv = r.vec3()?;
        let _bi = r.idx(r.bs, nbones.max(0) as usize, "softbody bone")?;
        let _nv = r.vec3()?;
        let _wg = r.u8()?;
        let _gl = r.u8()?;
    }
    let na = r.i32()?;
    check_count(na, "softbody anchors", file_len)?;
    for _ in 0..na {
        let _ai = r.idx(r.vs, nverts.max(0) as usize, "softbody anchor")?;
        let _mv = r.u8()?;
        let _gi = r.vec3()?;
        let _li = r.vec3()?;
        let _an = r.u8()?;
        let _vv = r.vec3()?;
        let _rv2 = r.vec3()?;
        let _rma = r.u32()?;
        let _lma = r.u32()?;
        let _sh = r.u8()?;
        let _hm = r.u8()?;
    }
    let np = r.i32()?;
    check_count(np, "softbody pins", file_len)?;
    for _ in 0..np {
        let _pi = r.idx(r.vs, nverts.max(0) as usize, "softbody pin")?;
    }
    Ok(())
}

fn check_count(v: i32, what: &str, file_len: usize) -> Result<()> {
    if v < 0 {
        return Err(MmdconvError::pmx_parse(0, format!("negative {what} count {v}")));
    }
    // each element occupies at least 1 byte; allow generous headroom
    if v as usize > file_len.saturating_mul(4).saturating_add(16) {
        return Err(MmdconvError::pmx_parse(0, format!("{what} count {v} implausible for a {file_len}-byte file")));
    }
    Ok(())
}

pub fn read_pmx_file(path: &std::path::Path) -> Result<PmxModel> {
    let data = std::fs::read(path)
        .map_err(|e| MmdconvError::io(format!("reading {}", path.display()), e))?;
    read_pmx(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_garbage() {
        assert!(read_pmx(b"not a pmx file at all").is_err());
        assert!(read_pmx(b"PMX ").is_err());
        assert!(read_pmx(&[]).is_err());
    }

    #[test]
    fn rejects_truncated() {
        let mut m = PmxModel::default();
        m.vertices = vec![PmxVertex::bdef(Vec3::X, &[(0, 1.0)])];
        m.faces = vec![0, 0, 0];
        m.bones = vec![PmxBone::default()];
        let bytes = crate::pmx::writer::write_pmx(&m, &crate::pmx::writer::WriterOpts::auto(&m)).unwrap();
        for cut in [bytes.len() - 1, bytes.len() / 2, 10] {
            assert!(read_pmx(&bytes[..cut]).is_err(), "cut {cut} should fail");
        }
        assert!(read_pmx(&bytes).is_ok());
    }

    #[test]
    fn fuzz_like_random_inputs_never_panic() {
        // deterministic LCG pseudo-random bytes
        let mut state: u64 = 0x12345678;
        let mut next = move || {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (state >> 33) as u8
        };
        for trial in 0..3000 {
            let len = 8 + (next() as usize) * 4;
            let mut buf: Vec<u8> = Vec::with_capacity(len);
            if trial % 3 == 0 {
                buf.extend_from_slice(b"PMX 3.1\0");
            }
            while buf.len() < len {
                buf.push(next());
            }
            let _ = read_pmx(&buf); // must not panic
        }
    }
}
