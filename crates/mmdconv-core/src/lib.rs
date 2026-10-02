//! mmdconv-core: format-agnostic IR, importers, PMX reader/writer/validator.
//!
//! Pipeline stages (see docs/ARCHITECTURE.md):
//! 1. `detect` — identify input format by magic bytes (not extension).
//! 2. `importers` — read a source file into [`ir::IrModel`].
//! 3. `convert` — normalize coordinates/scale and build a [`pmx::PmxModel`].
//! 4. `pmx` — write / validate / re-read the PMX binary.

pub mod error;
pub mod ir;
pub mod pmx;

pub mod detect {
    //! Magic-byte format detection. Extension is only a fallback hint.

    use std::path::Path;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Format {
        Glb,
        Gltf,
        Pmx,
        Pmd,
        Fbx,
        Dae,
        Obj,
        Stl,
        Ply,
        Unknown,
    }

    impl Format {
        pub fn label(self) -> &'static str {
            match self {
                Format::Glb => "glTF Binary (.glb/.vrm)",
                Format::Gltf => "glTF JSON (.gltf)",
                Format::Pmx => "PMX",
                Format::Pmd => "PMD",
                Format::Fbx => "FBX",
                Format::Dae => "Collada (.dae)",
                Format::Obj => "Wavefront OBJ",
                Format::Stl => "STL",
                Format::Ply => "PLY",
                Format::Unknown => "unknown",
            }
        }

        /// Whether this format is supported for conversion in this build.
        pub fn supported(self) -> bool {
            matches!(self, Format::Glb | Format::Gltf | Format::Pmx)
        }
    }

    /// Detect the format of `path` from its contents (magic bytes), falling
    /// back to the file extension when the content is textual/ambiguous.
    pub fn detect_file(path: &Path) -> Format {
        let data = match std::fs::read(path) {
            Ok(d) => d,
            Err(_) => return detect_by_extension(path),
        };
        let n = data.len().min(8192);
        detect_bytes(&data[..n]).unwrap_or_else(|| detect_by_extension(path))
    }

    pub fn detect_bytes(head: &[u8]) -> Option<Format> {
        if head.len() >= 8 && &head[0..4] == b"glTF" {
            return Some(Format::Glb);
        }
        if head.len() >= 4 && &head[0..4] == b"Kayd" {
            return Some(Format::Fbx); // FBX binary ("Kaydara FBX Binary  \0")
        }
        if head.len() >= 3 && &head[0..3] == b"ply" {
            return Some(Format::Ply);
        }
        // Text files: sniff the beginning.
        let s: String = head
            .iter()
            .map(|&c| if c.is_ascii() { c as char } else { ' ' })
            .collect();
        let t = s.trim_start();
        if t.starts_with("<?xml") {
            if head.windows(8).any(|w| w == b"COLLADA>") || head.windows(7).any(|w| w == b"COLLADA") {
                return Some(Format::Dae);
            }
            if head.windows(11).any(|w| w == b"fbxDocument") || head.windows(11).any(|w| w == b"FBXDocument") {
                return Some(Format::Fbx); // ASCII FBX
            }
            return None;
        }
        if t.starts_with('{')
            && (head.windows(7).any(|w| w == b"\"asset\"")
                || head.windows(8).any(|w| w == b"\"meshes\"")
                || head.windows(7).any(|w| w == b"\"nodes\""))
        {
            return Some(Format::Gltf);
        }
        if t.starts_with("v ") || t.starts_with("vt ") || t.starts_with("vn ") || t.starts_with("# ") {
            // OBJ-ish text; confirm first token of first line is v/vt/vn/f/o/g/usemtl/#
            if let Some(first) = t.lines().next() {
                let tok = first.split_whitespace().next().unwrap_or("");
                if matches!(tok, "v" | "vt" | "vn" | "f" | "o" | "g" | "usemtl" | "#") {
                    return Some(Format::Obj);
                }
            }
        }
        if t.starts_with("solid") {
            return Some(Format::Stl); // ASCII STL (binary STL handled by size check below)
        }
        None
    }

    /// Binary STL has no magic; identified by exact size equation. Callers may
    /// pass the full file length here.
    pub fn looks_like_binary_stl(len: usize, head: &[u8]) -> bool {
        if len < 84 || head.len() < 84 {
            return false;
        }
        let tris = u32::from_le_bytes([head[80], head[81], head[82], head[83]]) as usize;
        len == 84 + tris * 50
    }

    fn detect_by_extension(path: &Path) -> Format {
        match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
            Some("glb") | Some("vrm") => Format::Glb,
            Some("gltf") => Format::Gltf,
            Some("pmx") => Format::Pmx,
            Some("pmd") => Format::Pmd,
            Some("fbx") => Format::Fbx,
            Some("dae") => Format::Dae,
            Some("obj") => Format::Obj,
            Some("stl") => Format::Stl,
            Some("ply") => Format::Ply,
            _ => Format::Unknown,
        }
    }
}

/// Texture output format used when writing texture files beside the PMX.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureFormat {
    Png,
    Jpg,
    Tga,
    Bmp,
}

/// Conversion options shared by all pipeline entry points.
#[derive(Clone, Debug)]
pub struct ConvertOptions {
    /// Explicit scale multiplier applied on top of auto-scaling.
    pub scale: Option<f32>,
    /// Target model height in meters (default 1.58 m ≈ 19.75 MMD units).
    pub target_height_m: f32,
    /// Write edge (outline) settings on materials.
    pub edge: bool,
    /// Shared toon index 1..10 (None = no toon).
    pub toon: Option<u32>,
    /// Texture output format used when writing texture files.
    pub texture_format: TextureFormat,
    /// Directory name beside the output for textures.
    pub texture_dir: String,
    /// PMX version to emit.
    pub pmx_version: f32,
    /// Text encoding for the PMX header flag.
    pub encoding: pmx::PmxEncoding,
    /// Emit leg IK chains (requires humanoid mapping; ignored until M4).
    pub ik: bool,
    /// Emit helper bones (twist, shoulder-P, D-bones) where derivable.
    pub helper_bones: bool,
    /// Keep unmapped extra bones (hair etc.) attached to nearest parent.
    pub keep_unknown_bones: bool,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            scale: None,
            target_height_m: 1.58,
            edge: true,
            toon: None,
            texture_format: TextureFormat::Png,
            texture_dir: "tex".into(),
            pmx_version: 2.0,
            encoding: pmx::PmxEncoding::Utf16Le,
            ik: true,
            helper_bones: false,
            keep_unknown_bones: true,
        }
    }
}

pub mod convert {
    //! IR → PMX conversion (M2/M3 scope: geometry, skinning, materials,
    //! textures, vertex morphs, generic skeleton passthrough).
    //!
    //! Coordinate rule: RH→LH conversion mirrors X (negate x of positions,
    //! normals and morph deltas) and reverses triangle winding so front faces
    //! stay front-facing after the handedness flip. Z-up sources swap Y/Z.

    use crate::error::{MmdconvError, Result};
    use crate::ir::{IrModel, MorphTargetKind, PoseKind};
    use crate::pmx::{
        BoneTarget, IdSize, MorphPanel, PmxBone, PmxFrame, PmxFrameElement, PmxMaterial,
        PmxModel, PmxMorph, PmxMorphData, PmxText, PmxVertex, TailType,
    };
    use glam::Vec3;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};

    /// Outcome of a conversion: the PMX model plus report-style diagnostics.
    pub struct ConvertOutcome {
        pub model: PmxModel,
        pub warnings: Vec<String>,
        pub truncated_weight_vertices: usize,
        pub textures_written: Vec<PathBuf>,
    }

    pub fn ir_to_pmx(ir: &IrModel, opts: &crate::ConvertOptions, out_path: &Path) -> Result<ConvertOutcome> {
        let mut warnings = ir_warnings(ir);
        let mut truncated = 0usize;

        if ir.meshes.iter().all(|m| m.vertices.is_empty()) {
            return Err(MmdconvError::input("model has no vertices; nothing to convert"));
        }

        // ---- scale -----------------------------------------------------------
        let unit_m = ir.units_to_meters.unwrap_or(1.0);
        let bbox = ir_bbox(ir).expect("non-empty checked above");
        let up_span = (bbox.max.y - bbox.min.y).max(f32::EPSILON);
        let src_height_m = up_span * unit_m;
        let mut scale = opts.target_height_m / src_height_m;
        if let Some(s) = opts.scale {
            scale *= s;
        }
        if !scale.is_finite() || scale <= 0.0 {
            return Err(MmdconvError::input(format!("computed invalid scale factor {scale}")));
        }

        // ---- handedness / up-axis ---------------------------------------------
        let mirror_x = !ir.coord.is_left_handed(); // RH sources mirrored into LH PMX space
        let zup = ir.coord.up() == crate::ir::UpAxis::Z;
        if zup {
            warnings.push("source is Z-up; swapped Y/Z axes during conversion".into());
        }

        let xf = |p: Vec3| -> Vec3 {
            let mut v = if zup { Vec3::new(p.x, p.z, p.y) } else { p };
            if mirror_x {
                v.x = -v.x;
            }
            v * scale
        };
        let xf_delta = |d: Vec3| -> Vec3 {
            let mut v = if zup { Vec3::new(d.x, d.z, d.y) } else { d };
            if mirror_x {
                v.x = -v.x;
            }
            v * scale
        };
        let xf_dir = |n: Vec3| -> Vec3 {
            let mut v = if zup { Vec3::new(n.x, n.z, n.y) } else { n };
            if mirror_x {
                v.x = -v.x;
            }
            v.normalize_or_zero()
        };

        // ---- bones (generic passthrough; humanoid map lands in M4) --------------
        let nbones = ir.skeleton.bones.len();
        let mut bones: Vec<PmxBone> = Vec::with_capacity(nbones + 2);

        let mut root = PmxBone::default();
        root.name = PmxText::both("全ての親", "All Parent");
        root.position = Vec3::ZERO;
        root.tail = TailType::Offset(Vec3::ZERO);
        root.head_id = BoneTarget::Index(1); // センター
        root.flags |= crate::pmx::FLAG_TAIL_INDEX;
        root.level = 0;
        bones.push(root);

        let mut center = PmxBone::default();
        center.name = PmxText::both("センター", "Center");
        center.parent = Some(0);
        center.position = xf(avg_hips(ir));
        center.tail = TailType::Offset(Vec3::ZERO);
        center.level = 1;
        bones.push(center);

        if nbones == 0 {
            warnings.push("source has no skeleton; emitting mesh-only PMX with root bones only (BDEF1 rigid bind)".into());
        } else {
            warnings.push(
                "humanoid standard-skeleton mapping (MMD bone names, IK) is not implemented yet; \
                 preserving the original bone hierarchy under センター"
                    .into(),
            );
        }

        let mut bone_index: HashMap<u32, u32> = HashMap::new();
        for b in &ir.skeleton.bones {
            let idx = bones.len() as u32;
            bone_index.insert(b.id, idx);
            let mut pb = PmxBone::default();
            pb.name = dedup_name(&bones, PmxText::both(b.name.best(), &b.name.en));
            let pos = xf(b.global.transform_point3(Vec3::ZERO));
            pb.position = pos;
            pb.parent = Some(match b.parent {
                Some(p) => bone_index.get(&p).copied().unwrap_or(1),
                None => 1, // attach source roots under センター
            });
            pb.level = 2;
            pb.head_id = BoneTarget::Index(idx); // self-reference allowed by PMX spec
            if !b.children.is_empty() {
                let mut acc = Vec3::ZERO;
                let mut n = 0u32;
                for c in &b.children {
                    if let Some(cb) = ir.skeleton.bones.get(*c as usize) {
                        acc += xf(cb.global.transform_point3(Vec3::ZERO));
                        n += 1;
                    }
                }
                if n > 0 {
                    pb.tail = TailType::Offset(acc / n as f32 - pos);
                }
            }
            bones.push(pb);
        }

        // ---- vertices / faces / materials ----------------------------------------
        let has_uv1 = ir.meshes.iter().any(|m| m.vertices.iter().any(|v| v.uv1.is_some()));
        let mut pmx = PmxModel::default();
        pmx.version = opts.pmx_version;
        pmx.encoding = opts.encoding;
        pmx.additional_uv_count = if has_uv1 { 1 } else { 0 };
        pmx.name = PmxText::both(
            if ir.meta.name.jp.is_empty() { file_stem(out_path) } else { ir.meta.name.jp.clone() },
            if ir.meta.name.en.is_empty() { file_stem(out_path) } else { ir.meta.name.en.clone() },
        );
        pmx.comment = build_comment(ir);

        let mut tex_paths: Vec<String> = Vec::new();
        let mut written_tex: HashMap<u32, String> = HashMap::new();
        let mut tex_written: Vec<PathBuf> = Vec::new();

        let mut base_vertex = 0u32;
        let mut mat_face_counts: Vec<i64> = Vec::new();
        for (mi, mesh) in ir.meshes.iter().enumerate() {
            let attachment_bone = ir
                .mesh_attachments
                .iter()
                .find(|(mid, _)| *mid == mi as u32)
                .and_then(|(_, bid)| bone_index.get(bid).copied());

            for v in &mesh.vertices {
                if v.weights.len() > 4 {
                    truncated += 1;
                }
                let mut ws: Vec<(u32, f32)> = v
                    .weights
                    .iter()
                    .take(4)
                    .filter_map(|w| bone_index.get(&w.bone).copied().map(|b| (b, w.weight)))
                    .filter(|(_, w)| *w > 0.0)
                    .collect();
                if ws.is_empty() {
                    ws.push((attachment_bone.unwrap_or(0), 1.0));
                }
                let sum: f32 = ws.iter().map(|(_, w)| *w).sum();
                if sum > 0.0 {
                    for (_, w) in ws.iter_mut() {
                        *w /= sum;
                    }
                } else {
                    ws = vec![(attachment_bone.unwrap_or(0), 1.0)];
                }
                let mut pv = PmxVertex::bdef(xf(v.pos), &ws);
                pv.normal = xf_dir(v.normal);
                pv.uv = v.uv;
                if pmx.additional_uv_count > 0 {
                    pv.extra_uv = vec![match v.uv1 {
                        Some(u) => [u[0], u[1], 0.0, 0.0],
                        None => [0.0; 4],
                    }];
                }
                pmx.vertices.push(pv);
            }

            for prim in &mesh.primitives {
                let mat_idx = ensure_material(
                    ir,
                    prim.material,
                    opts,
                    &mut pmx,
                    &mut mat_face_counts,
                    &mut tex_paths,
                    &mut written_tex,
                    &mut tex_written,
                    out_path,
                )?;
                if prim.indices.len() % 3 != 0 {
                    warnings.push(format!("mesh #{mi}: primitive had {} indices (not divisible by 3); trailing indices dropped", prim.indices.len()));
                }
                for tri in prim.indices.chunks(3) {
                    if tri.len() != 3 {
                        continue;
                    }
                    if mirror_x {
                        pmx.faces.push(base_vertex + tri[0]);
                        pmx.faces.push(base_vertex + tri[2]);
                        pmx.faces.push(base_vertex + tri[1]);
                    } else {
                        pmx.faces.push(base_vertex + tri[0]);
                        pmx.faces.push(base_vertex + tri[1]);
                        pmx.faces.push(base_vertex + tri[2]);
                    }
                    mat_face_counts[mat_idx as usize] += 3;
                }
            }
            base_vertex += mesh.vertices.len() as u32;
        }

        // ---- morphs ---------------------------------------------------------------
        if !ir.morphs.is_empty() {
            let mut mesh_vertex_base: Vec<u32> = Vec::new();
            let mut acc = 0u32;
            for m in &ir.meshes {
                mesh_vertex_base.push(acc);
                acc += m.vertices.len() as u32;
            }
            for mo in &ir.morphs {
                if mo.kind != MorphTargetKind::Vertex {
                    warnings.push(format!(
                        "morph '{}': {:?} morphs are not supported yet and were skipped",
                        mo.name.best(),
                        mo.kind
                    ));
                    continue;
                }
                let Some(&base) = mesh_vertex_base.get(mo.mesh as usize) else {
                    warnings.push(format!("morph '{}': references unknown mesh #{}", mo.name.best(), mo.mesh));
                    continue;
                };
                let offs: Vec<(u32, Vec3)> = mo
                    .offsets
                    .iter()
                    .map(|o| (base + o.vertex, xf_delta(o.offset)))
                    .collect();
                if offs.is_empty() {
                    warnings.push(format!("morph '{}': empty offset list skipped", mo.name.best()));
                    continue;
                }
                pmx.morphs.push(PmxMorph {
                    name: PmxText::both(mo.name.best(), &mo.name.en),
                    panel: MorphPanel::Other,
                    offset_kind: 1, // vertex
                    data: PmxMorphData::Vertex(offs),
                });
            }
        }

        // ---- display frames ---------------------------------------------------------
        let mut frame = PmxFrame { name: PmxText::both("骨骼フレーム", "Bones"), is_special: false, elements: Vec::new() };
        for i in 0..bones.len() {
            frame.elements.push(PmxFrameElement { is_morph: false, index: i as u32 });
        }
        pmx.display_frames.push(frame);
        if !pmx.morphs.is_empty() {
            let mut mf = PmxFrame { name: PmxText::both("表情", "Expressions"), is_special: false, elements: Vec::new() };
            for i in 0..pmx.morphs.len() {
                mf.elements.push(PmxFrameElement { is_morph: true, index: i as u32 });
            }
            pmx.display_frames.push(mf);
        }

        let has_textures = !tex_paths.is_empty();
        pmx.textures = tex_paths;
        pmx.bones = bones;

        // ---- index sizes --------------------------------------------------------------
        pmx.vertex_size = IdSize::for_count(pmx.vertices.len().max(1));
        pmx.texture_size = IdSize::for_count(pmx.textures.len());
        pmx.material_size = IdSize::for_count(pmx.materials.len());
        pmx.bone_size = IdSize::for_count(pmx.bones.len().max(1));
        pmx.morph_size = IdSize::for_count(pmx.morphs.len());
        pmx.rigid_body_size = IdSize::for_count(pmx.rigid_bodies.len());

        if truncated > 0 {
            warnings.push(format!("{truncated} vertices had more than 4 bone influences; kept the 4 strongest and renormalized"));
        }
        if !pmx.materials.is_empty() && !has_any_texture(ir) && has_textures {
            // informational only
        }

        Ok(ConvertOutcome { model: pmx, warnings, truncated_weight_vertices: truncated, textures_written: tex_written })
    }

    // ---------------------------------------------------------------------- helpers

    #[derive(Clone, Copy)]
    struct BBox {
        min: Vec3,
        max: Vec3,
    }

    fn ir_bbox(ir: &IrModel) -> Option<BBox> {
        let mut it = ir.meshes.iter().flat_map(|m| m.vertices.iter()).map(|v| v.pos);
        let first = it.next()?;
        let mut min = first;
        let mut max = first;
        for p in it {
            min = min.min(p);
            max = max.max(p);
        }
        if ir.coord.up() == crate::ir::UpAxis::Z {
            // measure height along the actual up axis
            let sw = |v: Vec3| Vec3::new(v.x, v.z, v.y);
            let (a, b) = (sw(min), sw(max));
            min = Vec3::new(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z));
            max = Vec3::new(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z));
        }
        Some(BBox { min, max })
    }

    fn avg_hips(ir: &IrModel) -> Vec3 {
        for b in &ir.skeleton.bones {
            if b.semantic == crate::ir::SemanticRole::Hips {
                return b.global.transform_point3(Vec3::ZERO);
            }
        }
        ir.skeleton
            .bones
            .iter()
            .filter(|b| b.kind == crate::ir::BoneKind::Joint)
            .map(|b| b.global.transform_point3(Vec3::ZERO))
            .min_by(|a, b| {
                let ka = (a.x.to_bits(), a.y.to_bits(), a.z.to_bits());
                let kb = (b.x.to_bits(), b.y.to_bits(), b.z.to_bits());
                ka.cmp(&kb)
            })
            .unwrap_or(Vec3::ZERO)
    }

    fn file_stem(p: &Path) -> String {
        p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "model".into())
    }

    fn build_comment(ir: &IrModel) -> PmxText {
        let mut lines = vec![format!(
            "Converted by mmdconv {} from {}",
            env!("CARGO_PKG_VERSION"),
            ir.meta.source_file.display()
        )];
        if !ir.meta.author.is_empty() {
            lines.push(format!("Author (source): {}", ir.meta.author));
        }
        if !ir.meta.license.is_empty() {
            lines.push(format!("License (source): {}", ir.meta.license));
        }
        if ir.pose == PoseKind::TPose {
            lines.push("Source pose: T-pose (A-pose rebinding not yet implemented)".to_string());
        }
        PmxText::new(lines.join("\n"))
    }

    fn has_any_texture(ir: &IrModel) -> bool {
        !ir.textures.is_empty()
    }

    fn ir_warnings(ir: &IrModel) -> Vec<String> {
        let mut w = Vec::new();
        if ir.pose == PoseKind::TPose {
            w.push("source appears to be in T-pose; --pose a normalization is not implemented yet (milestone M5), rest pose kept as-is".into());
        }
        for m in &ir.materials {
            if !m.extras_ignored.is_empty() {
                w.push(format!("material '{}': ignored extensions: {}", m.name.best(), m.extras_ignored.join(", ")));
            }
        }
        if !ir.springs.is_empty() {
            w.push(format!(
                "{} spring-bone chains found but physics generation is not implemented yet (milestone M8); springs were dropped",
                ir.springs.len()
            ));
        }
        w
    }

    fn dedup_name(existing: &[PmxBone], mut name: PmxText) -> PmxText {
        if name.name.is_empty() {
            name.name = "bone".into();
        }
        let taken: Vec<&str> = existing.iter().map(|b| b.name.name.as_str()).collect();
        if !taken.contains(&name.name.as_str()) {
            return name;
        }
        let base = name.name.clone();
        let mut i = 2;
        loop {
            let cand = format!("{base}{i}");
            if !taken.contains(&cand.as_str()) {
                name.name = cand;
                return name;
            }
            i += 1;
        }
    }

    /// Map an IR material to a PMX material slot (deduplicated by IR id via memo).
    #[allow(clippy::too_many_arguments)]
    fn ensure_material(
        ir: &IrModel,
        mat_id: u32,
        opts: &crate::ConvertOptions,
        pmx: &mut PmxModel,
        mat_face_counts: &mut Vec<i64>,
        tex_paths: &mut Vec<String>,
        written_tex: &mut HashMap<u32, String>,
        tex_written: &mut Vec<PathBuf>,
        out_path: &Path,
    ) -> Result<u32> {
        if let Some(pos) = pmx.materials.iter().position(|m| m.memo == format!("ir-material:{mat_id}")) {
            return Ok(pos as u32);
        }
        let src = ir
            .materials
            .get(mat_id as usize)
            .ok_or_else(|| MmdconvError::input(format!("primitive references missing material #{mat_id}")))?;
        let mut m = PmxMaterial::default();
        m.name = PmxText::both(src.name.best(), &src.name.en);
        m.memo = format!("ir-material:{mat_id}");
        let bc = src.pbr.base_color;
        // PBR → MMD approximation (see DECISIONS.md):
        //   diffuse   = base color × alpha-mode handling
        //   specular  = (1-roughness)(1-metallic) strength
        //   shininess = (1-roughness) × 128
        //   ambient   = diffuse × 0.1 + emissive × strength
        let rough = src.pbr.roughness.clamp(0.0, 1.0);
        let metal = src.pbr.metallic.clamp(0.0, 1.0);
        m.shininess = ((1.0 - rough) * 128.0).max(1.0);
        let spec = (1.0 - rough) * (1.0 - metal) * 0.5;
        m.diffuse = [bc[0], bc[1], bc[2], alpha_of(src)];
        m.specular = [spec, spec, spec];
        let es = src.pbr.emissive_strength.max(0.0);
        m.ambient = [
            bc[0] * 0.1 + src.pbr.emissive[0] * es,
            bc[1] * 0.1 + src.pbr.emissive[1] * es,
            bc[2] * 0.1 + src.pbr.emissive[2] * es,
        ];
        m.double_sided = src.double_sided || matches!(src.alpha, crate::ir::AlphaMode::Blend);
        m.draw_edge = opts.edge;
        match opts.toon {
            Some(t) => {
                m.shared_toon = true;
                m.toon = Some(t.clamp(1, 10) - 1);
            }
            None => {
                m.shared_toon = false;
                m.toon = None;
            }
        }
        if let Some(tid) = src.pbr.base_color_texture {
            if let Some(rel) = write_texture(ir, tid, out_path, opts, written_tex, tex_written)? {
                match tex_paths.iter().position(|x| *x == rel) {
                    Some(i) => m.texture = Some(i as u32),
                    None => {
                        tex_paths.push(rel);
                        m.texture = Some(tex_paths.len() as u32 - 1);
                    }
                }
            }
        }
        pmx.materials.push(m);
        mat_face_counts.push(0);
        Ok(pmx.materials.len() as u32 - 1)
    }

    fn alpha_of(m: &crate::ir::Material) -> f32 {
        match m.alpha {
            crate::ir::AlphaMode::Opaque => 1.0,
            crate::ir::AlphaMode::Mask(_) | crate::ir::AlphaMode::Blend => m.pbr.base_color[3],
        }
    }

    fn sanitize_asset_name(name: &str, ext: &str) -> String {
        let mut s: String = name
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' })
            .collect();
        while s.contains("__") {
            s = s.replace("__", "_");
        }
        s = s.trim_matches('_').to_string();
        if s.is_empty() {
            s = "texture".into();
        }
        // strip any original extension, then append ours
        if let Some(dot) = s.rfind('.') {
            if s[dot..].chars().all(|c| c.is_ascii_alphabetic()) && s[dot..].len() <= 5 {
                s.truncate(dot);
            }
        }
        format!("{s}.{ext}")
    }

    /// Encode one IR texture into the side-car directory; returns the relative
    /// path (forward slashes — valid on Windows too) or None if undecodable.
    fn write_texture(
        ir: &IrModel,
        tid: u32,
        out_path: &Path,
        opts: &crate::ConvertOptions,
        written: &mut HashMap<u32, String>,
        tex_written: &mut Vec<PathBuf>,
    ) -> Result<Option<String>> {
        let tex = match ir.textures.get(tid as usize) {
            Some(t) => t,
            None => return Ok(None),
        };
        if let Some(p) = written.get(&tid) {
            return Ok(Some(p.clone()));
        }
        let ext = match opts.texture_format {
            crate::TextureFormat::Png => "png",
            crate::TextureFormat::Jpg => "jpg",
            crate::TextureFormat::Tga => "tga",
            crate::TextureFormat::Bmp => "bmp",
        };
        let fname = sanitize_asset_name(&tex.name, ext);
        let dir = out_path.parent().unwrap_or(Path::new(".")).join(&opts.texture_dir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| MmdconvError::io(format!("creating texture dir {}", dir.display()), e))?;
        let full = dir.join(&fname);
        let img = image::RgbaImage::from_raw(tex.width, tex.height, tex.pixels.clone())
            .ok_or_else(|| MmdconvError::input(format!("texture '{}' has inconsistent size data", tex.name)))?;
        let fmt = match opts.texture_format {
            crate::TextureFormat::Png => image::ImageFormat::Png,
            crate::TextureFormat::Jpg => image::ImageFormat::Jpeg,
            crate::TextureFormat::Tga => image::ImageFormat::Tga,
            crate::TextureFormat::Bmp => image::ImageFormat::Bmp,
        };
        let mut sink = std::io::Cursor::new(Vec::<u8>::new());
        if opts.texture_format == crate::TextureFormat::Jpg {
            let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
            rgb.write_to(&mut sink, fmt)
        } else {
            img.write_to(&mut sink, fmt)
        }
        .map_err(|e| MmdconvError::input(format!("encoding texture '{}': {e}", tex.name)))?;
        let buf = sink.into_inner();
        std::fs::write(&full, &buf).map_err(|e| MmdconvError::io(full.display().to_string(), e))?;
        let rel = format!("{}/{}", opts.texture_dir, fname);
        written.insert(tid, rel.clone());
        tex_written.push(full);
        Ok(Some(rel))
    }
}

/// Importers: source file → [`ir::IrModel`].
pub mod importers {
    pub mod common;
    pub mod gltf;
}
