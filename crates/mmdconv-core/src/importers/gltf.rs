//! glTF 2.0 / GLB importer (JSON chunk parsed with our own strict parser) and
//! VRM 0.x / 1.0 extension handling (humanoid map, expressions, spring bones).

use super::common::*;
use crate::error::{MmdconvError, Result};
use crate::ir::*;
use glam::{Mat4, Quat, Vec3};
use std::path::Path;

const COMPONENT_TYPES: [(&str, usize); 7] = [
    ("SCALAR", 1), ("VEC2", 2), ("VEC3", 3), ("VEC4", 4),
    ("MAT2", 4), ("MAT3", 9), ("MAT4", 16),
];

fn component_byte_len(comp: u32) -> Option<usize> {
    match comp {
        5120 => Some(1),  // BYTE
        5121 => Some(1),  // UNSIGNED_BYTE
        5122 => Some(2),  // SHORT
        5123 => Some(2),  // UNSIGNED_SHORT
        5125 => Some(4),  // UNSIGNED_INT
        5126 => Some(4),  // FLOAT
        _ => None,
    }
}

fn type_count(ty: &str) -> Option<usize> {
    COMPONENT_TYPES.iter().find(|(t, _)| *t == ty).map(|(_, n)| *n)
}

/// A buffer source: either embedded in the GLB BIN chunk or an external file.
pub struct BufferProvider<'a> {
    base_dir: &'a Path,
    bin_chunk: Option<&'a [u8]>,
    cache: std::collections::HashMap<usize, Vec<u8>>,
}

impl<'a> BufferProvider<'a> {
    pub fn new(base_dir: &'a Path, bin_chunk: Option<&'a [u8]>) -> Self {
        BufferProvider { base_dir, bin_chunk, cache: Default::default() }
    }
    fn buffer(&mut self, idx: usize, uris: &[Option<String>]) -> Result<&[u8]> {
        if !self.cache.contains_key(&idx) {
            let data = match uris.get(idx).and_then(|o| o.as_ref()) {
                Some(uri) => {
                    if let Some(rest) = uri.strip_prefix("data:") {
                        let b64 = rest.rsplit(',').next().ok_or_else(|| MmdconvError::input("bad data URI"))?;
                        base64_decode(b64)?
                    } else {
                        let p = percent_decode(uri);
                        let path = self.base_dir.join(p.trim_start_matches("./"));
                        std::fs::read(&path).map_err(|e| MmdconvError::io(format!("reading buffer {}", path.display()), e))?
                    }
                }
                None => {
                    if idx != 0 {
                        return Err(MmdconvError::input(format!("buffer {idx} has no URI and no GLB BIN chunk")));
                    }
                    self.bin_chunk.ok_or_else(|| MmdconvError::input("GLB missing BIN chunk"))?.to_vec()
                }
            };
            self.cache.insert(idx, data);
        }
        Ok(self.cache.get(&idx).unwrap().as_slice())
    }

    /// Read accessor `idx` as f32 values (converting integer types by dividing
    /// normalized attributes appropriately per glTF spec).
    pub fn accessor_f32(
        &mut self,
        json: &JsonValue,
        idx: usize,
        normalized_divisor: Option<f32>,
    ) -> Result<Vec<f32>> {
        let accessors = json.get("accessors").and_then(|a| a.as_array()).ok_or_else(|| MmdconvError::input("glTF: no accessors"))?;
        let acc = accessors.get(idx).ok_or_else(|| MmdconvError::input(format!("glTF: accessor {idx} out of range")))?;
        let bv_idx = acc.get("bufferView").and_then(|v| v.as_usize_index())
            .ok_or_else(|| MmdconvError::input("glTF: sparse/unsupported accessor (no bufferView)"))?;
        if acc.get("sparse").is_some() {
            return Err(MmdconvError::input("glTF: sparse accessors are not supported"));
        }
        let bvs = json.get("bufferViews").and_then(|v| v.as_array()).ok_or_else(|| MmdconvError::input("glTF: no bufferViews"))?;
        let bv = bvs.get(bv_idx).ok_or_else(|| MmdconvError::input(format!("glTF: bufferView {bv_idx} out of range")))?;
        let buf_idx = bv.get("buffer").and_then(|b| b.as_usize_index()).unwrap_or(0);
        let byte_offset = bv.get("byteOffset").and_then(|b| b.as_f64()).unwrap_or(0.0) as usize
            + acc.get("byteOffset").and_then(|b| b.as_f64()).unwrap_or(0.0) as usize;
        let comp = acc.get("componentType").and_then(|c| c.as_f64()).unwrap_or(-1.0) as u32;
        let cb = component_byte_len(comp).ok_or_else(|| MmdconvError::input(format!("glTF: bad componentType {comp}")))?;
        let ty = acc.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let n = type_count(ty).ok_or_else(|| MmdconvError::input(format!("glTF: bad accessor type '{ty}'")))?;
        let count = acc.get("count").and_then(|c| c.as_f64()).unwrap_or(-1.0) as usize;
        if count > 64_000_000 {
            return Err(MmdconvError::input(format!("glTF: accessor count {count} implausible")));
        }
        let stride = bv.get("byteStride").and_then(|s| s.as_f64()).unwrap_or(0.0) as usize;
        let step = if stride == 0 { cb * n } else { stride };
        let uris: Vec<Option<String>> = json
            .get("buffers")
            .and_then(|b| b.as_array())
            .map(|arr| arr.iter().map(|b| b.get("uri").and_then(|u| u.as_str()).map(String::from)).collect())
            .unwrap_or_default();
        let data = self.buffer(buf_idx, &uris)?;
        let total = cb * n;
        let mut out = Vec::with_capacity(count * n);
        for i in 0..count {
            let base = byte_offset + i * step;
            if base + total > data.len() {
                return Err(MmdconvError::input(format!(
                    "glTF: accessor {idx} element {i} extends past buffer (need {} bytes, have {})",
                    base + total, data.len())));
            }
            let raw = &data[base..base + total];
            for c in 0..n {
                let sl = &raw[c * cb..(c + 1) * cb];
                let v: f32 = match comp {
                    5126 => f32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]),
                    5121 => sl[0] as f32,
                    5120 => sl[0] as i8 as f32,
                    5123 => u16::from_le_bytes([sl[0], sl[1]]) as f32,
                    5122 => i16::from_le_bytes([sl[0], sl[1]]) as f32,
                    5125 => u32::from_le_bytes([sl[0], sl[1], sl[2], sl[3]]) as f32,
                    _ => unreachable!(),
                };
                out.push(match normalized_divisor {
                    Some(d) => v / d,
                    None => v,
                });
            }
        }
        Ok(out)
    }

    /// Read index accessor as u32 list.
    pub fn accessor_indices(&mut self, json: &JsonValue, idx: usize) -> Result<Vec<u32>> {
        let accessors = json.get("accessors").and_then(|a| a.as_array()).unwrap();
        let acc = &accessors[idx];
        let comp = acc.get("componentType").and_then(|c| c.as_f64()).unwrap_or(-1.0) as u32;
        let vals = self.accessor_f32(json, idx, None)?;
        Ok(match comp {
            5121 | 5123 | 5125 => vals.iter().map(|v| *v as u32).collect(),
            other => return Err(MmdconvError::input(format!("glTF: bad index componentType {other}"))),
        })
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn base64_val(c: u8) -> Option<u8> {
    match c {
        b'A'..=b'Z' => Some(c - b'A'),
        b'a'..=b'z' => Some(c - b'a' + 26),
        b'0'..=b'9' => Some(c - b'0' + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

pub fn base64_decode(s: &str) -> Result<Vec<u8>> {
    let bytes: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    let mut quad = [0u8; 4];
    let mut qi = 0;
    for b in bytes {
        if b == b'=' {
            break;
        }
        quad[qi] = base64_val(b).ok_or_else(|| MmdconvError::input("bad base64 character"))?;
        qi += 1;
        if qi == 4 {
            out.push((quad[0] << 2) | (quad[1] >> 4));
            out.push((quad[1] << 4) | (quad[2] >> 2));
            out.push((quad[2] << 6) | quad[3]);
            qi = 0;
        }
    }
    if qi >= 2 {
        out.push((quad[0] << 2) | (quad[1] >> 4));
        if qi >= 3 {
            out.push((quad[1] << 4) | (quad[2] >> 2));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// GLB container
// ---------------------------------------------------------------------------

pub struct GlbParts<'a> {
    pub json: &'a [u8],
    pub bin: Option<&'a [u8]>,
}

pub fn parse_glb(data: &[u8]) -> Result<GlbParts<'_>> {
    if data.len() < 12 || &data[0..4] != b"glTF" {
        return Err(MmdconvError::input("not a GLB file (magic mismatch)"));
    }
    let version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    if version != 2 {
        return Err(MmdconvError::input(format!("unsupported glTF binary version {version}")));
    }
    let total = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
    if total > data.len() {
        return Err(MmdconvError::input(format!("GLB header claims {total} bytes, file has {}", data.len())));
    }
    let mut p = 12usize;
    let mut json: Option<&[u8]> = None;
    let mut bin: Option<&[u8]> = None;
    while p + 8 <= total {
        let clen = u32::from_le_bytes([data[p], data[p + 1], data[p + 2], data[p + 3]]) as usize;
        let ctype = &data[p + 4..p + 8];
        p += 8;
        if p + clen > total {
            return Err(MmdconvError::input("GLB chunk extends past declared length"));
        }
        if ctype == b"JSON" {
            if json.is_some() {
                return Err(MmdconvError::input("GLB has multiple JSON chunks"));
            }
            json = Some(&data[p..p + clen]);
        } else if ctype == b"BIN\0" {
            if bin.is_some() {
                return Err(MmdconvError::input("GLB has multiple BIN chunks"));
            }
            bin = Some(&data[p..p + clen]);
        }
        p += clen;
    }
    let json = json.ok_or_else(|| MmdconvError::input("GLB missing JSON chunk"))?;
    Ok(GlbParts { json, bin })
}

// ---------------------------------------------------------------------------
// main glTF import
// ---------------------------------------------------------------------------

pub fn import_gltf_json(
    json: &JsonValue,
    provider_base: &Path,
    bin_chunk: Option<&[u8]>,
    warnings: &mut Vec<String>,
) -> Result<IrModel> {
    let asset = json.get("asset");
    if let Some(a) = asset {
        if let Some(v) = a.get("version").and_then(|v| v.as_str()) {
            if !v.starts_with('2') {
                return Err(MmdconvError::input(format!("glTF version {v} is not 2.x")));
            }
        }
    } else {
        return Err(MmdconvError::input("glTF: missing 'asset' object"));
    }
    let mut model = IrModel::empty();
    model.coord = CoordSystem::RhYup; // glTF is always Y-up right-handed, meters
    model.units_to_meters = Some(1.0);
    let mut bp = BufferProvider::new(provider_base, bin_chunk);

    // ---- metadata ----
    if let Some(name) = json.get("scene").and_then(|_| json.get("scenes")).and_then(|s| s.as_array()).and_then(|arr| arr.first()).and_then(|s| s.get("name")).and_then(|n| n.as_str()) {
        model.meta.name.jp = name.to_string();
    }
    if let Some(gen) = asset.and_then(|a| a.get("generator")).and_then(|g| g.as_str()) {
        model.meta.comment.en = gen.to_string();
    }
    if let Some(c) = asset.and_then(|a| a.get("copyright")).and_then(|g| g.as_str()) {
        model.meta.author = c.to_string();
    }
    // license hints from extensionsUsed names
    if let Some(ext) = json.get("extensionsUsed").and_then(|e| e.as_array()) {
        for e in ext {
            if let Some(s) = e.as_str() {
                if s.starts_with("VRM") || s.starts_with("VRChat") {
                    model.meta.license.push_str(s);
                    model.meta.license.push(' ');
                }
            }
        }
    }

    // ---- nodes → skeleton ----
    let nodes = json.get("nodes").and_then(|n| n.as_array_owned()).unwrap_or_default();
    let skins = json.get("skins").and_then(|s| s.as_array_owned()).unwrap_or_default();
    let meshes_json = json.get("meshes").and_then(|m| m.as_array_owned()).unwrap_or_default();

    // node → bone id (all nodes become bones in IR; kind distinguishes them)
    let mut node_bone: Vec<BoneId> = vec![u32::MAX; nodes.len()];
    // topological order: DFS from scenes' root nodes (and any unreferenced nodes)
    let mut ordered: Vec<usize> = Vec::with_capacity(nodes.len());
    let mut seen = vec![false; nodes.len()];
    let mut roots_referenced: Vec<usize> = Vec::new();
    if let Some(scenes) = json.get("scenes").and_then(|s| s.as_array()) {
        let active = json.get("scene").and_then(|s| s.as_usize_index()).unwrap_or(0);
        if let Some(sc) = scenes.get(active) {
            if let Some(nns) = sc.get("nodes").and_then(|n| n.as_array()) {
                for n in nns {
                    if let Some(i) = n.as_usize_index() {
                        if i < nodes.len() {
                            roots_referenced.push(i);
                        }
                    }
                }
            }
        }
    }
    if roots_referenced.is_empty() {
        // find nodes that are nobody's child
        let mut is_child = vec![false; nodes.len()];
        for n in &nodes {
            if let Some(ch) = n.get("children").and_then(|c| c.as_array()) {
                for c in ch {
                    if let Some(i) = c.as_usize_index() {
                        if i < nodes.len() {
                            is_child[i] = true;
                        }
                    }
                }
            }
        }
        roots_referenced = (0..nodes.len()).filter(|i| !is_child[*i]).collect();
    }
    {
        let mut stack: Vec<usize> = roots_referenced.iter().rev().copied().collect();
        while let Some(ni) = stack.pop() {
            if seen[ni] {
                continue;
            }
            seen[ni] = true;
            ordered.push(ni);
            if let Some(ch) = nodes[ni].get("children").and_then(|c| c.as_array()) {
                for c in ch.iter().rev() {
                    if let Some(i) = c.as_usize_index() {
                        if i < nodes.len() && !seen[i] {
                            stack.push(i);
                        }
                    }
                }
            }
        }
        // orphan/cyclic nodes appended at end attached to root
        for i in 0..nodes.len() {
            if !seen[i] {
                ordered.push(i);
            }
        }
    }
    for (bid, &ni) in ordered.iter().enumerate() {
        let n = &nodes[ni];
        let mut bone = Bone::new(bid as BoneId, LText::jp(n.get("name").and_then(|s| s.as_str()).unwrap_or("").to_string()));
        if let Some(trs) = n.get("translation").and_then(|t| t.as_f32_vec()) {
            if trs.len() == 3 {
                bone.translation = Vec3::from_slice(&trs);
            }
        }
        if let Some(q) = n.get("rotation").and_then(|t| t.as_f32_vec()) {
            if q.len() == 4 {
                bone.rotation = Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize();
            }
        }
        if let Some(s) = n.get("scale").and_then(|t| t.as_f32_vec()) {
            if s.len() == 3 {
                bone.scale = Vec3::from_slice(&s);
            }
        }
        if n.get("mesh").is_none() {
            bone.kind = BoneKind::Joint;
        }
        node_bone[ni] = bid as BoneId;
        model.skeleton.bones.push(bone);
    }
    // parent/child links
    let mut has_parent = vec![false; ordered.len()];
    for (bi, &ni) in ordered.iter().enumerate() {
        if let Some(ch) = nodes[ni].get("children").and_then(|c| c.as_array()) {
            for c in ch {
                if let Some(ci) = c.as_usize_index() {
                    if ci < nodes.len() {
                        let cbo = ordered.iter().position(|&x| x == ci).map(|x| x as BoneId);
                        if let Some(c) = cbo {
                            model.skeleton.bones[c as usize].parent = Some(bi as BoneId);
                            model.skeleton.bones[c as usize].children.push(bi as BoneId);
                            has_parent[c as usize] = true;
                        }
                    }
                }
            }
        }
    }
    model.skeleton.roots = (0..ordered.len() as BoneId).filter(|i| !has_parent[*i as usize]).collect();
    model.skeleton.update_global_transforms();

    // skin joint-node → bone-id map (per skin)
    let mut skin_joint_bones: Vec<Vec<BoneId>> = Vec::with_capacity(skins.len());
    for sk in &skins {
        let mut joints = Vec::new();
        if let Some(js) = sk.get("joints").and_then(|j| j.as_array()) {
            for j in js {
                if let Some(ni) = j.as_usize_index() {
                    joints.push(if ni < nodes.len() { node_bone[ni] } else { u32::MAX });
                } else {
                    joints.push(u32::MAX);
                }
            }
        }
        skin_joint_bones.push(joints);
    }

    // ---- materials ----
    let mats_json = json.get("materials").and_then(|m| m.as_array_owned()).unwrap_or_default();
    let textures_json = json.get("textures").and_then(|t| t.as_array_owned()).unwrap_or_default();
    let images_json = json.get("images").and_then(|i| i.as_array_owned()).unwrap_or_default();
    let mut image_cache: Vec<Option<TextureId>> = vec![None; images_json.len()];
    let mut tex_path_cache: std::collections::HashMap<String, TextureId> = Default::default();

    for (mi, mj) in mats_json.iter().enumerate() {
        let mut mat = Material {
            name: LText::jp(mj.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string()),
            pbr: PbrFactor {
                base_color: [1.0, 1.0, 1.0, 1.0],
                metallic: 1.0,
                roughness: 1.0,
                emissive_strength: 1.0,
                normal_scale: 1.0,
                occlusion_strength: 1.0,
                ..Default::default()
            },
            alpha: AlphaMode::Opaque,
            double_sided: mj.get("doubleSided").and_then(|d| d.as_bool()).unwrap_or(false),
            emissive_texture: None,
            extras_ignored: Vec::new(),
        };
        if let Some(pmr) = mj.get("pbrMetallicRoughness") {
            if let Some(bc) = pmr.get("baseColorFactor").and_then(|b| b.as_f32_vec()) {
                if bc.len() == 4 {
                    mat.pbr.base_color = [bc[0], bc[1], bc[2], bc[3]];
                }
            }
            mat.pbr.metallic = pmr.get("metallicFactor").and_then(|m| m.as_f64()).unwrap_or(1.0) as f32;
            mat.pbr.roughness = pmr.get("roughnessFactor").and_then(|m| m.as_f64()).unwrap_or(1.0) as f32;
            if let Some(t) = pmr.get("baseColorTexture").and_then(|t| t.get("index")).and_then(|i| i.as_usize_index()) {
                mat.pbr.base_color_texture = load_texture(t, &textures_json, &mut bp, json, &mut image_cache, &mut tex_path_cache, warnings, TexUsage::Color, &mut model.textures)?;
            }
            if let Some(t) = pmr.get("metallicRoughnessTexture").and_then(|t| t.get("index")).and_then(|i| i.as_usize_index()) {
                mat.pbr.metallic_roughness_texture = load_texture(t, &textures_json, &mut bp, json, &mut image_cache, &mut tex_path_cache, warnings, TexUsage::MetallicRoughness, &mut model.textures)?;
            }
        }
        if let Some(e) = mj.get("emissiveFactor").and_then(|b| b.as_f32_vec()) {
            if e.len() == 3 {
                mat.pbr.emissive = [e[0], e[1], e[2]];
            }
        }
        if let Some(t) = mj.get("emissiveTexture").and_then(|t| t.get("index")).and_then(|i| i.as_usize_index()) {
            mat.emissive_texture = load_texture(t, &textures_json, &mut bp, json, &mut image_cache, &mut tex_path_cache, warnings, TexUsage::Emissive, &mut model.textures)?;
        }
        match mj.get("alphaMode").and_then(|a| a.as_str()).unwrap_or("OPAQUE") {
            "BLEND" => mat.alpha = AlphaMode::Blend,
            "MASK" => mat.alpha = AlphaMode::Mask(mj.get("alphaCutoff").and_then(|c| c.as_f64()).unwrap_or(0.5) as f32),
            _ => mat.alpha = AlphaMode::Opaque,
        }
        // KHR_materials_* handling / warnings
        if let Some(exts) = mj.get("extensions") {
            if let JsonValue::Obj(items) = exts {
                for (k, _) in items {
                    match k.as_str() {
                        "KHR_materials_emissive_strength" => {}
                        "KHR_materials_specular" | "KHR_materials_ior" | "KHR_materials_clearcoat"
                        | "KHR_materials_transmission" | "KHR_materials_volume" | "KHR_materials_pbrSpecularGlossiness" => {
                            mat.extras_ignored.push(k.clone());
                        }
                        _ => {}
                    }
                }
            }
        }
        if mj.get("normalTexture").is_some() {
            warnings.push(format!("material {mi}: normal maps are ignored (PMX has no tangent space in this pipeline)"));
        }
        if mj.get("occlusionTexture").is_some() {
            warnings.push(format!("material {mi}: AO texture ignored"));
        }
        model.materials.push(mat);
    }
    if model.materials.is_empty() {
        model.materials.push(Material {
            name: LText::jp("default"),
            pbr: PbrFactor { base_color: [0.8, 0.8, 0.8, 1.0], metallic: 0.0, roughness: 0.9, emissive_strength: 1.0, normal_scale: 1.0, occlusion_strength: 1.0, ..Default::default() },
            alpha: AlphaMode::Opaque,
            double_sided: false,
            emissive_texture: None,
            extras_ignored: vec![],
        });
    }

    // ---- meshes ----
    let mut mesh_json_to_ir: Vec<Option<MeshId>> = vec![None; meshes_json.len()];
    for (ni, n) in nodes.iter().enumerate() {
        let Some(mesh_idx) = n.get("mesh").and_then(|m| m.as_usize_index()) else { continue };
        let Some(mj) = meshes_json.get(mesh_idx) else {
            warnings.push(format!("node {ni} references missing mesh {mesh_idx}"));
            continue;
        };
        // find skin containing this node
        let skin_idx = skins.iter().position(|sk| sk.get("skeleton").and_then(|s| s.as_usize_index()) == Some(ni))
            .or_else(|| skins.iter().position(|sk| {
                sk.get("joints").and_then(|j| j.as_array()).map(|js| js.iter().any(|j| j.as_usize_index() == Some(ni))).unwrap_or(false)
            }));
        if mesh_json_to_ir[mesh_idx].is_some() {
            // shared mesh instance: duplicate (weights differ per skin otherwise)
            warnings.push(format!("mesh {mesh_idx} instanced on multiple nodes; geometry duplicated"));
        }
        let mut mesh = Mesh {
            name: mj.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string(),
            vertices: Vec::new(),
            primitives: Vec::new(),
            inverse_bind: Vec::new(),
        };
        let joints = skin_idx.map(|s| skin_joint_bones[s].clone()).unwrap_or_default();
        if let Some(si) = skin_idx {
            if let Some(ibm) = skins[si].get("inverseBindMatrices").and_then(|i| i.as_usize_index()) {
                let floats = bp.accessor_f32(json, ibm, None)?;
                for c in floats.chunks_exact(16) {
                    mesh.inverse_bind.push(Mat4::from_cols_array(c.try_into().unwrap()));
                }
            }
        }
        for prim in mj.get("primitives").and_then(|p| p.as_array_owned()).unwrap_or_default() {
            let attrs = prim.get("attributes").cloned().unwrap_or(JsonValue::Obj(vec![]));
            let pos_acc = attrs.get("POSITION").and_then(|a| a.as_usize_index())
                .ok_or_else(|| MmdconvError::input(format!("mesh {mesh_idx}: primitive without POSITION")))?;
            let positions = bp.accessor_f32(json, pos_acc, None)?;
            let normals = match attrs.get("NORMAL").and_then(|a| a.as_usize_index()) {
                Some(a) => Some(bp.accessor_f32(json, a, None)?),
                None => None,
            };
            let uvs0 = match attrs.get("TEXCOORD_0").and_then(|a| a.as_usize_index()) {
                Some(a) => Some(bp.accessor_f32(json, a, Some(1.0))?),
                None => None,
            };
            let uvs1 = match attrs.get("TEXCOORD_1").and_then(|a| a.as_usize_index()) {
                Some(a) => Some(bp.accessor_f32(json, a, Some(1.0))?),
                None => None,
            };
            let joints_acc = attrs.get("JOINTS_0").and_then(|a| a.as_usize_index());
            let weights_acc = attrs.get("WEIGHTS_0").and_then(|a| a.as_usize_index());
            let (joint_data, weight_data) = match (joints_acc, weights_acc) {
                (Some(j), Some(w)) => (bp.accessor_f32(json, j, None)?, bp.accessor_f32(json, w, Some(1.0))?),
                _ => (Vec::new(), Vec::new()),
            };
            let nverts = positions.len() / 3;
            let base = mesh.vertices.len() as u32;
            for v in 0..nverts {
                let mut vert = Vertex {
                    pos: Vec3::from_slice(&positions[v * 3..v * 3 + 3]),
                    normal: normals.as_ref().map(|nn| Vec3::from_slice(&nn[v * 3..v * 3 + 3])).unwrap_or(Vec3::Y),
                    uv: uvs0.as_ref().map(|u| [u[v * 2], u[v * 2 + 1]]).unwrap_or([0.0; 2]),
                    uv1: uvs1.as_ref().map(|u| [u[v * 2], u[v * 2 + 1]]),
                    weights: Vec::new(),
                };
                if !joint_data.is_empty() {
                    for k in 0..4 {
                        let jref = joint_data[v * 4 + k] as usize;
                        let wgt = weight_data[v * 4 + k];
                        if wgt > 1e-6 {
                            let bone = joints.get(jref).copied().unwrap_or(u32::MAX);
                            if bone != u32::MAX {
                                vert.weights.push(Weight { bone, weight: wgt });
                            }
                        }
                    }
                }
                mesh.vertices.push(vert);
            }
            let material = prim.get("material").and_then(|m| m.as_usize_index()).unwrap_or(0).min(model.materials.len() - 1) as MaterialId;
            let indices: Vec<u32> = match prim.get("indices").and_then(|i| i.as_usize_index()) {
                Some(ia) => {
                    let raw = bp.accessor_indices(json, ia)?;
                    raw.into_iter().map(|x| x + base).collect()
                }
                None => (0..nverts as u32).map(|x| x + base).collect(),
            };
            let mode = prim.get("mode").and_then(|m| m.as_f64()).unwrap_or(4.0) as u32;
            let tris = match mode {
                4 => indices,
                5 | 6 => {
                    warnings.push(format!("mesh {mesh_idx}: triangle strip/fan (mode {mode}) converted to list"));
                    if mode == 5 {
                        indices.windows(3).map(|w| vec![w[0], w[1], w[2]]).flatten().collect()
                    } else {
                        let mut t = Vec::new();
                        for win in indices.windows(3) {
                            t.extend_from_slice(&[win[0], win[1], win[2]]);
                        }
                        t
                    }
                }
                _ => {
                    warnings.push(format!("mesh {mesh_idx}: non-triangle primitive mode {mode} dropped"));
                    Vec::new()
                }
            };
            if !tris.is_empty() {
                mesh.primitives.push(Primitive { material, indices: tris });
            }
        }
        if mesh.vertices.is_empty() {
            continue;
        }
        if normals_missing(&mesh) {
            compute_smooth_normals(&mut mesh);
        }
        // attach unskinned meshes rigidly to their node bone
        let attachment = node_bone.get(ni).copied().unwrap_or(u32::MAX);
        let mid = model.meshes.len() as MeshId;
        mesh_json_to_ir[mesh_idx] = Some(mid);
        model.meshes.push(mesh);
        if joints.is_empty() && attachment != u32::MAX {
            model.mesh_attachments.push((mid, attachment));
        }
    }
    let _ = &mut mesh_json_to_ir;

    // ---- morph targets ----
    for (ni, n) in nodes.iter().enumerate() {
        let Some(mesh_idx) = n.get("mesh").and_then(|m| m.as_usize_index()) else { continue };
        let Some(target_mid) = mesh_json_to_ir.get(mesh_idx).and_then(|x| *x) else { continue };
        let Some(mj) = meshes_json.get(mesh_idx) else { continue };
        let prims = match mj.get("primitives").and_then(|p| p.as_array()) {
            Some(p) => p,
            None => continue,
        };
        for (ti, target) in mj.get("targets").and_then(|t| t.as_array_owned()).unwrap_or_default().iter().enumerate() {
            let name = target.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
            let mut offsets = Vec::new();
            let mut ok = true;
            let mut vertex_base = 0usize;
            for prim in prims {
                let Some(pa) = prim.get("attributes").and_then(|a| a.get("POSITION")).and_then(|p| p.as_usize_index()) else {
                    continue;
                };
                let nverts = json.get("accessors").and_then(|a| a.as_array()).and_then(|arr| arr.get(pa)).and_then(|a| a.get("count")).and_then(|c| c.as_usize_index()).unwrap_or(0);
                let Some(tpos_acc) = target.get("POSITION").and_then(|p| p.as_usize_index()) else {
                    ok = false;
                    break;
                };
                let deltas = match bp.accessor_f32(json, tpos_acc, None) {
                    Ok(d) => d,
                    Err(_) => { ok = false; break; }
                };
                for v in 0..nverts {
                    if v * 3 + 2 >= deltas.len() {
                        break;
                    }
                    let d = Vec3::from_slice(&deltas[v * 3..v * 3 + 3]);
                    if d.length_squared() > 1e-12 {
                        offsets.push(MorphOffset { vertex: (vertex_base + v) as VertexId, offset: d });
                    }
                }
                vertex_base += nverts;
            }
            if !ok || offsets.is_empty() {
                if !ok {
                    warnings.push(format!("node {ni}: morph target {ti} skipped (incomplete attributes)"));
                }
                continue;
            }
            model.morphs.push(MorphTarget {
                id: model.morphs.len() as MorphId,
                name: LText::jp(name.clone()),
                kind: MorphTargetKind::Vertex,
                mesh: target_mid,
                offsets,
                source_hint: name,
            });
        }
    }

    // ---- VRM humanoid / expressions / springs ----
    if let Some(ext) = json.get("extensions") {
        import_vrm_extensions(ext, &node_bone, &mut model, warnings);
    }
    let _ = textures_json;

    // pose detection (T vs A) — done after skeleton globals exist
    detect_pose(&mut model);
    Ok(model)
}

fn normals_missing(mesh: &Mesh) -> bool {
    mesh.vertices.iter().all(|v| v.normal == Vec3::Y)
}

#[allow(clippy::too_many_arguments)]
fn load_texture(
    tex_idx: usize,
    textures_json: &[JsonValue],
    bp: &mut BufferProvider,
    json: &JsonValue,
    image_cache: &mut Vec<Option<TextureId>>,
    path_cache: &mut std::collections::HashMap<String, TextureId>,
    warnings: &mut Vec<String>,
    usage: TexUsage,
    textures: &mut Vec<Texture>,
) -> Result<Option<TextureId>> {
    let Some(img_idx) = textures_json.get(tex_idx).and_then(|t| t.get("source")).and_then(|s| s.as_usize_index()) else {
        return Ok(None);
    };
    if img_idx >= image_cache.len() {
        warnings.push(format!("texture {tex_idx} references missing image {img_idx}"));
        return Ok(None);
    }
    if let Some(id) = image_cache[img_idx] {
        return Ok(Some(id));
    }
    let images = json.get("images").and_then(|i| i.as_array_owned()).unwrap_or_default();
    let img = &images[img_idx];
    let data = if let Some(view_idx) = img.get("bufferView").and_then(|b| b.as_usize_index()) {
        let bvs = json.get("bufferViews").and_then(|v| v.as_array_owned()).unwrap_or_default();
        let bv = bvs.get(view_idx).ok_or_else(|| MmdconvError::input("image bufferView out of range"))?;
        let buf_idx = bv.get("buffer").and_then(|b| b.as_usize_index()).unwrap_or(0);
        let off = bv.get("byteOffset").and_then(|b| b.as_f64()).unwrap_or(0.0) as usize;
        let len = bv.get("byteLength").and_then(|b| b.as_f64()).unwrap_or(0.0) as usize;
        let uris: Vec<Option<String>> = json.get("buffers").and_then(|b| b.as_array())
            .map(|arr| arr.iter().map(|b| b.get("uri").and_then(|u| u.as_str()).map(String::from)).collect())
            .unwrap_or_default();
        let all = bp_buffer_slice(bp, json, buf_idx, &uris)?;
        if off + len > all.len() {
            return Err(MmdconvError::input("image extends past buffer"));
        }
        all[off..off + len].to_vec()
    } else if let Some(uri) = img.get("uri").and_then(|u| u.as_str()) {
        if let Some(rest) = uri.strip_prefix("data:") {
            let b64 = rest.rsplit(',').next().ok_or_else(|| MmdconvError::input("bad data URI"))?;
            base64_decode(b64)?
        } else {
            let p = percent_decode(uri);
            let path = bp.base_dir().join(p.trim_start_matches("./"));
            std::fs::read(&path).map_err(|e| MmdconvError::io(format!("reading image {}", path.display()), e))?
        }
    } else {
        warnings.push(format!("image {img_idx} has no data"));
        return Ok(None);
    };
    let name = img.get("name").and_then(|n| n.as_str()).unwrap_or("texture").to_string();
    let decoded = image::load_from_memory(&data)
        .map_err(|e| MmdconvError::input(format!("decoding embedded image {img_idx} ({name}): {e}")))?;
    let rgba = decoded.to_rgba8();
    let (w, h) = rgba.dimensions();
    let id = textures.len() as TextureId;
    textures.push(Texture {
        id,
        name,
        usage,
        pixels: rgba.into_raw(),
        width: w,
        height: h,
        srgb: matches!(usage, TexUsage::Color | TexUsage::Emissive),
    });
    image_cache[img_idx] = Some(id);
    path_cache.insert(format!("{img_idx}"), id);
    Ok(Some(id))
}

fn bp_buffer_slice<'x>(bp: &'x mut BufferProvider, json: &JsonValue, buf_idx: usize, uris: &[Option<String>]) -> Result<&'x [u8]> {
    let _ = json;
    bp.buffer(buf_idx, uris)
}

impl<'a> BufferProvider<'a> {
    pub fn base_dir(&self) -> &'a Path {
        self.base_dir
    }
}

fn import_vrm_extensions(
    ext: &JsonValue,
    node_bone: &[BoneId],
    model: &mut IrModel,
    warnings: &mut Vec<String>,
) {
    // VRM 1.0: extensions.VRM.humanoid... ; VRM 0.x under "VRM" too
    let vrm = ext.get("VRM").or_else(|| ext.get("vrm"));
    let Some(vrm) = vrm else { return };
    model.humanoid_map_present = true;
    let meta_ver = vrm.get("meta").and_then(|m| m.get("specVersion")).and_then(|s| s.as_str()).unwrap_or("").to_string();
    if meta_ver.is_empty() {
        warnings.push("VRM specVersion missing; assuming 1.0".to_string());
    }

    // humanoid bone map
    if let Some(hb) = vrm.get("humanoid").and_then(|h| h.get("humanBones")).and_then(|a| a.as_array()) {
        for entry in hb {
            let ty = entry.get("bone").and_then(|b| b.as_str()).unwrap_or("");
            let node_i = entry.get("node").and_then(|n| n.as_usize_index());
            let Some(ni) = node_i else { continue };
            let Some(&bid) = node_bone.get(ni) else { continue };
            if let Some(role) = vrm_bone_to_role(ty) {
                let b = &mut model.skeleton.bones[bid as usize];
                b.semantic = role;
                b.semantic_confidence = 1.0;
                b.tags.push(format!("vrm:{ty}"));
            }
        }
    }
    // meta fields
    if let Some(meta) = vrm.get("meta") {
        let g = |k: &str| meta.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !g("name").is_empty() { model.meta.name.jp = g("name"); }
        model.meta.author = g("authors");
        if model.meta.author.is_empty() {
            if let Some(authors) = meta.get("authors").and_then(|a| a.as_array()) {
                model.meta.author = authors.iter().filter_map(|a| a.as_str()).collect::<Vec<_>>().join(", ");
            }
        }
        model.meta.license = g("licenseUrl");
        if model.meta.license.is_empty() {
            model.meta.license = g("otherLicenseUrl");
        }
        model.meta.comment.jp = g("description");
        model.meta.comment.en = g("description");
    }
    // expressions: record preset names + associated morph indices on bones
    if let Some(preset) = vrm.get("expressions").and_then(|e| e.get("preset")) {
        if let JsonValue::Obj(items) = preset {
            for (name, entry) in items {
                if let Some(binds) = entry.get("morphTargetBinds").and_then(|m| m.as_array()) {
                    let mut offs: Vec<(VertexId, Vec3)> = Vec::new();
                    for b in binds {
                        let _ = b; // mesh-attribute-level binding requires node lookup; captured by name below
                    }
                    let _ = &mut offs;
                    model.vrm_expressions.push((name.clone(), entry.clone()));
                } else {
                    model.vrm_expressions.push((name.clone(), entry.clone()));
                }
            }
        }
    }
    // spring bones (physics)
    let springs_json = vrm
        .get("springBone")
        .and_then(|s| s.get("springs"))
        .and_then(|a| a.as_array_owned())
        .or_else(|| vrm.get("springBone").and_then(|s| s.as_array_owned()))
        .unwrap_or_default();
    for sp in springs_json {
        let mut sb = SpringBone {
            joint: u32::MAX,
            center: None,
            axis: Vec3::ZERO,
            gravity_power: sp.get("gravityPower").and_then(|g| g.as_f64()).unwrap_or(0.5) as f32,
            stiffness: sp.get("stiffiness").or_else(|| sp.get("stiffness")).and_then(|g| g.as_f64()).unwrap_or(0.5) as f32,
            hit_radius_scale: sp.get("hitRadiusScale").and_then(|g| g.as_f64()).unwrap_or(1.0) as f32,
            pull_back: sp.get("pullGravityPower").and_then(|g| g.as_f64()).unwrap_or(0.5) as f32,
            front_limit: 0.0, rear_limit: 0.0, left_limit: 0.0, right_limit: 0.0, down_limit: 0.0, up_limit: 0.0,
            colliders: Vec::new(),
        };
        if let Some(c) = sp.get("center").and_then(|c| c.as_usize_index()) {
            sb.center = node_bone.get(c).copied();
        }
        if let Some(cols) = sp.get("colliderGroups").and_then(|c| c.as_array()) {
            for cg in cols {
                let _ = cg; // collider group resolution below when available
            }
        }
        for j in sp.get("bones").and_then(|b| b.as_array_owned()).unwrap_or_default() {
            let Some(ni) = j.get("bone").and_then(|b| b.as_usize_index()).or_else(|| j.as_usize_index()) else { continue };
            if let Some(&bid) = node_bone.get(ni) {
                if sb.joint == u32::MAX {
                    sb.joint = bid;
                }
                if let Some(r) = j.get("rotateGravityPower").and_then(|r| r.as_f64()) {
                    sb.gravity_power = sb.gravity_power.max(r as f32);
                }
            }
        }
        if sb.joint != u32::MAX {
            model.springs.push(sb);
        }
    }
}

pub fn vrm_bone_to_role(ty: &str) -> Option<SemanticRole> {
    Some(match ty {
        "Hips" => SemanticRole::Hips,
        "Spine" => SemanticRole::Spine,
        "Chest" => SemanticRole::Chest,
        "UpperChest" => SemanticRole::Spine2,
        "Neck" => SemanticRole::Neck,
        "Head" => SemanticRole::Head,
        "LeftEye" => SemanticRole::LeftEye,
        "RightEye" => SemanticRole::RightEye,
        "Jaw" => SemanticRole::Jaw,
        "LeftShoulder" => SemanticRole::LeftShoulder,
        "LeftUpperArm" => SemanticRole::LeftUpperArm,
        "LeftLowerArm" => SemanticRole::LeftLowerArm,
        "LeftHand" => SemanticRole::LeftHand,
        "RightShoulder" => SemanticRole::RightShoulder,
        "RightUpperArm" => SemanticRole::RightUpperArm,
        "RightLowerArm" => SemanticRole::RightLowerArm,
        "RightHand" => SemanticRole::RightHand,
        "LeftThumbMetacarpal" => SemanticRole::LeftThumbProximal,
        "LeftThumbProximal" => SemanticRole::LeftThumbIntermediate,
        "LeftThumbDistal" => SemanticRole::LeftThumbDistal,
        "RightThumbMetacarpal" => SemanticRole::RightThumbProximal,
        "RightThumbProximal" => SemanticRole::RightThumbIntermediate,
        "RightThumbDistal" => SemanticRole::RightThumbDistal,
        "LeftIndexProximal" => SemanticRole::LeftIndexProximal,
        "LeftIndexIntermediate" => SemanticRole::LeftIndexIntermediate,
        "LeftIndexDistal" => SemanticRole::LeftIndexDistal,
        "LeftMiddleProximal" => SemanticRole::LeftMiddleProximal,
        "LeftMiddleIntermediate" => SemanticRole::LeftMiddleIntermediate,
        "LeftMiddleDistal" => SemanticRole::LeftMiddleDistal,
        "LeftRingProximal" => SemanticRole::LeftRingProximal,
        "LeftRingIntermediate" => SemanticRole::LeftRingIntermediate,
        "LeftRingDistal" => SemanticRole::LeftRingDistal,
        "LeftLittleProximal" => SemanticRole::LeftLittleProximal,
        "LeftLittleIntermediate" => SemanticRole::LeftLittleIntermediate,
        "LeftLittleDistal" => SemanticRole::LeftLittleDistal,
        "RightIndexProximal" => SemanticRole::RightIndexProximal,
        "RightIndexIntermediate" => SemanticRole::RightIndexIntermediate,
        "RightIndexDistal" => SemanticRole::RightIndexDistal,
        "RightMiddleProximal" => SemanticRole::RightMiddleProximal,
        "RightMiddleIntermediate" => SemanticRole::RightMiddleIntermediate,
        "RightMiddleDistal" => SemanticRole::RightMiddleDistal,
        "RightRingProximal" => SemanticRole::RightRingProximal,
        "RightRingIntermediate" => SemanticRole::RightRingIntermediate,
        "RightRingDistal" => SemanticRole::RightRingDistal,
        "RightLittleProximal" => SemanticRole::RightLittleProximal,
        "RightLittleIntermediate" => SemanticRole::RightLittleIntermediate,
        "RightLittleDistal" => SemanticRole::RightLittleDistal,
        "LeftUpperLeg" => SemanticRole::LeftUpLeg,
        "LeftLowerLeg" => SemanticRole::LeftLeg,
        "LeftFoot" => SemanticRole::LeftFoot,
        "LeftToes" => SemanticRole::LeftToeBase,
        "RightUpperLeg" => SemanticRole::RightUpLeg,
        "RightLowerLeg" => SemanticRole::RightLeg,
        "RightFoot" => SemanticRole::RightFoot,
        "RightToes" => SemanticRole::RightToeBase,
        _ => return None,
    })
}

/// Heuristic T-pose / A-pose detection using upper-arm direction in rest pose.
pub fn detect_pose(model: &mut IrModel) {
    if model.pose != PoseKind::Unknown {
        return;
    }
    model.skeleton.update_global_transforms();
    let hips_y = model.skeleton.bones.iter().find(|b| b.semantic == SemanticRole::Hips).map(|b| b.global.transform_point3(Vec3::ZERO).y);
    let Some(hy) = hips_y else { return };
    for side in [SemanticRole::LeftUpperArm, SemanticRole::RightUpperArm] {
        if let Some(ua) = model.skeleton.bones.iter().find(|b| b.semantic == side) {
            if let Some(child) = ua.children.iter().find_map(|c| model.skeleton.bones.get(*c as usize)) {
                let pa = ua.global.transform_point3(Vec3::ZERO);
                let pb = child.global.transform_point3(Vec3::ZERO);
                let d = pb - pa;
                let horiz = Vec3::new(d.x, 0.0, d.z).length();
                if horiz < 1e-6 {
                    continue;
                }
                let angle_below_horizon = (-d.y).atan2(horiz);
                if pa.y > hy && angle_below_horizon > 0.15 {
                    model.pose = PoseKind::APose;
                    return;
                } else if pa.y > hy && angle_below_horizon.abs() < 0.15 {
                    model.pose = PoseKind::TPose;
                    return;
                }
            }
        }
    }
}

/// Import a .glb binary blob.
pub fn import_glb(data: &[u8], base_dir: &Path, warnings: &mut Vec<String>) -> Result<IrModel> {
    let parts = parse_glb(data)?;
    let json_src = std::str::from_utf8(parts.json)
        .map_err(|_| MmdconvError::input("GLB JSON chunk is not valid UTF-8"))?;
    let json = parse_json(json_src)?;
    import_gltf_json(&json, base_dir, parts.bin, warnings)
}

/// Import a .gltf text file (buffers resolved relative to its directory).
pub fn import_gltf_file(path: &Path, warnings: &mut Vec<String>) -> Result<IrModel> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| MmdconvError::io(format!("reading {}", path.display()), e))?;
    let json = parse_json(&src)?;
    let base = path.parent().unwrap_or(Path::new("."));
    import_gltf_json(&json, base, None, warnings)
}
