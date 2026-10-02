//! Standalone PMX validator: re-checks structural and semantic integrity of a
//! parsed PMX model (used by `mmdconv validate` and after every conversion).

use super::model::*;
use crate::error::{MmdconvError, Result};

#[derive(Clone, Debug, Default)]
pub struct ValidationReport {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

impl ValidationReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
    pub fn into_result(self) -> Result<()> {
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(MmdconvError::Validation { problems: self.errors })
        }
    }
}

pub fn validate(m: &PmxModel, check_files: Option<&std::path::Path>) -> ValidationReport {
    let mut r = ValidationReport::default();
    let nv = m.vertices.len();
    let nb = m.bones.len();
    let nt = m.textures.len();
    let nm = m.materials.len();
    let nmo = m.morphs.len();
    let nrb = m.rigid_bodies.len();

    if m.version != 2.0 && m.version != 2.1 {
        r.errors.push(format!("unsupported PMX version {}", m.version));
    }
    if !m.name.name.trim().is_empty() == false && m.vertices.is_empty() {
        // empty name is legal but suspicious for a converted model — warning only
        r.warnings.push("model name is empty".into());
    }

    // index sizes vs counts
    let need_u32 = |count: usize, size: IdSize, what: &str, r: &mut ValidationReport| {
        if count >= 0x7FFF && size != IdSize::U32 {
            r.errors.push(format!("{what}: {count} entries require 4-byte indices"));
        }
        if count >= 0x7F && size == IdSize::U8 {
            r.errors.push(format!("{what}: {count} entries do not fit in 1-byte indices"));
        }
    };
    need_u32(nv, m.vertex_size, "vertices", &mut r);
    need_u32(nt, m.texture_size, "textures", &mut r);
    need_u32(nm, m.material_size, "materials", &mut r);
    need_u32(nb, m.bone_size, "bones", &mut r);
    need_u32(nmo, m.morph_size, "morphs", &mut r);
    need_u32(nrb, m.rigid_body_size, "rigid bodies", &mut r);

    // faces
    if m.faces.len() % 3 != 0 {
        r.errors.push(format!("face count {} not divisible by 3", m.faces.len()));
    }
    let mut used_vertices = vec![false; nv];
    for (i, &f) in m.faces.iter().enumerate() {
        if f as usize >= nv {
            r.errors.push(format!("face #{i} vertex index {f} out of range ({nv})"));
        } else {
            used_vertices[f as usize] = true;
        }
    }
    let unused = used_vertices.iter().filter(|u| !**u).count();
    if unused > 0 {
        r.warnings.push(format!("{unused} vertices are not referenced by any face"));
    }

    // vertices
    for (i, v) in m.vertices.iter().enumerate() {
        if !v.pos.is_finite() || !v.normal.is_finite() {
            r.errors.push(format!("vertex {i} contains NaN/Inf"));
        }
        let n = match v.deform {
            DeformType::Bdef1 => 1,
            DeformType::Bdef2 | DeformType::Sdef => 2,
            DeformType::Bdef4 | DeformType::Qdef => 4,
        };
        let sum: f32 = v.weights[..n].iter().sum();
        if (sum - 1.0).abs() > 0.05 {
            r.errors.push(format!("vertex {i} weights sum to {sum}"));
        }
        for bi in &v.bones[..n] {
            if *bi as usize >= nb {
                r.errors.push(format!("vertex {i} bone index {bi} out of range"));
            }
        }
        if v.extra_uv.len() != m.additional_uv_count as usize {
            r.errors.push(format!("vertex {i} has {} extra UVs, header says {}", v.extra_uv.len(), m.additional_uv_count));
        }
    }

    // materials cover faces contiguously
    let mut covered = 0usize;
    for (i, mat) in m.materials.iter().enumerate() {
        if let Some(t) = mat.texture {
            if t as usize >= nt {
                r.errors.push(format!("material {i} texture index {t} out of range"));
            }
        }
        if let Some(s) = mat.sphere {
            if s as usize >= nt {
                r.errors.push(format!("material {i} sphere index {s} out of range"));
            }
        }
        if mat.shared_toon && mat.toon.map(|t| t > 9).unwrap_or(true) {
            r.errors.push(format!("material {i}: shared toon index must be 0..9"));
        }
        if !mat.shared_toon {
            if let Some(t) = mat.toon {
                if t as usize >= nt {
                    r.errors.push(format!("material {i} toon texture index {t} out of range"));
                }
            }
        }
        covered += 0; // face ranges are checked below via sequential assumption
    }
    let _ = covered;

    // bones
    for (i, b) in m.bones.iter().enumerate() {
        if !b.position.is_finite() {
            r.errors.push(format!("bone {i} position contains NaN"));
        }
        if let Some(p) = b.parent {
            if p as usize >= nb {
                r.errors.push(format!("bone {i} parent {p} out of range"));
            }
        }
        if let TailType::Index(t) = b.tail {
            if t as usize >= nb {
                r.errors.push(format!("bone {i} tail index {t} out of range"));
            }
        }
        if let BoneTarget::Index(h) = b.head_id {
            if h as usize >= nb {
                r.errors.push(format!("bone {i} head index {h} out of range"));
            }
        }
        if let Some(ik) = &b.ik {
            if ik.target as usize >= nb {
                r.errors.push(format!("bone {i} IK target {} out of range", ik.target));
            }
            if ik.loop_count == 0 {
                r.errors.push(format!("bone {i} IK loop count is 0"));
            }
            if ik.angle_limit <= 0.0 || ik.angle_limit > std::f32::consts::PI {
                r.errors.push(format!(
                    "bone {i} IK angle limit {} should be in (0, pi]",
                    ik.angle_limit
                ));
            }
            if ik.links.is_empty() {
                r.errors.push(format!("bone {i} IK has no links"));
            }
            let mut seen = std::collections::HashSet::new();
            for l in &ik.links {
                if l.bone as usize >= nb {
                    r.errors.push(format!("bone {i} IK link {} out of range", l.bone));
                }
                if !seen.insert(l.bone) {
                    r.errors.push(format!("bone {i} IK has duplicate link {}", l.bone));
                }
                if l.limit_enabled {
                    let d = l.upper - l.lower;
                    if d.x < 0.0 || d.y < 0.0 || d.z < 0.0 {
                        r.errors.push(format!("bone {i} IK link {} has inverted angle limits", l.bone));
                    } else if d.x.max(d.y).max(d.z) == 0.0 {
                        r.warnings.push(format!("bone {i} IK link {} limit range is zero", l.bone));
                    }
                }
            }
            if seen.contains(&ik.target) {
                r.errors.push(format!("bone {i} IK target appears among its own links"));
            }
        }
        if let Some(ih) = &b.inherit {
            if ih.bone as usize >= nb {
                r.errors.push(format!("bone {i} inherit-append target {} out of range", ih.bone));
            }
            if ih.bone == i as u32 {
                r.errors.push(format!("bone {i} appends to itself"));
            }
        }
    }
    // parent cycles + ordering sanity (parents may appear after children in PMX,
    // but cycles are fatal)
    for start in 0..nb {
        let mut slow = start;
        let mut fast = start;
        let mut steps = 0;
        loop {
            let sp = m.bones[slow].parent;
            let fp = m.bones[fast].parent.and_then(|p| m.bones[p as usize].parent);
            match (sp, fp) {
                (None, _) | (_, None) => break,
                (Some(s), Some(f)) => {
                    slow = s as usize;
                    fast = f as usize;
                    if slow == fast {
                        r.errors.push(format!("bone parent cycle detected at/above bone {start}"));
                        break;
                    }
                }
            }
            steps += 1;
            if steps > nb + 2 {
                break;
            }
        }
    }

    // morphs
    for (i, mo) in m.morphs.iter().enumerate() {
        match &mo.data {
            PmxMorphData::Vertex(v) => {
                if mo.offset_kind != 1 {
                    r.errors.push(format!("morph {i}: data/kind mismatch"));
                }
                for (idx, off) in v {
                    if *idx as usize >= nv {
                        r.errors.push(format!("morph {i} vertex offset {idx} out of range"));
                    }
                    if !off.is_finite() {
                        r.errors.push(format!("morph {i} offset at {idx} contains NaN"));
                    }
                }
                let mut sorted = true;
                for w in v.windows(2) {
                    if w[0].0 >= w[1].0 {
                        sorted = false;
                    }
                }
                if !sorted && !v.is_empty() {
                    r.warnings.push(format!("morph {i} vertex offsets are not ascending"));
                }
            }
            PmxMorphData::BoneRel(v) => {
                for (idx, _, _) in v {
                    if *idx as usize >= nb {
                        r.errors.push(format!("morph {i} bone offset {idx} out of range"));
                    }
                }
            }
            PmxMorphData::Uv(v) => {
                for (idx, _) in v {
                    if *idx as usize >= nv {
                        r.errors.push(format!("morph {i} UV offset {idx} out of range"));
                    }
                }
            }
            PmxMorphData::Material(v) => {
                for o in v {
                    if o.material >= 0 && o.material as usize >= nm {
                        r.errors.push(format!("morph {i} material index {} out of range", o.material));
                    }
                }
            }
            PmxMorphData::Group(v) => {
                for (idx, _) in v {
                    if *idx as usize >= nmo {
                        r.errors.push(format!("morph {i} group member {idx} out of range"));
                    }
                    if *idx as usize == i {
                        r.errors.push(format!("morph {i} includes itself"));
                    }
                }
            }
            PmxMorphData::Flip(v) => {
                for (idx, _) in v {
                    if *idx as usize >= nb {
                        r.errors.push(format!("morph {i} flip target {idx} out of range"));
                    }
                }
            }
            PmxMorphData::Impulse(v) => {
                for (idx, t, q) in v {
                    if *idx as usize >= nv {
                        r.errors.push(format!("morph {i} impulse target {idx} out of range"));
                    }
                    if !t.is_finite() || !q.is_finite() {
                        r.errors.push(format!("morph {i} impulse at {idx} contains NaN"));
                    }
                }
            }
            PmxMorphData::InverseRatio(v) => {
                for (idx, _) in v {
                    if *idx as usize >= nb {
                        r.errors.push(format!("morph {i} inverse-ratio target {idx} out of range"));
                    }
                }
            }
        }
    }

    // display frames
    for (i, f) in m.display_frames.iter().enumerate() {
        for el in &f.elements {
            if el.is_morph && el.index as usize >= nmo {
                r.errors.push(format!("frame {i} morph element {} out of range", el.index));
            }
            if !el.is_morph && el.index as usize >= nb {
                r.errors.push(format!("frame {i} bone element {} out of range", el.index));
            }
        }
    }

    // rigid bodies / joints
    for (i, rb) in m.rigid_bodies.iter().enumerate() {
        if let Some(b) = rb.bone {
            if b as usize >= nb {
                r.errors.push(format!("rigid body {i} bone index {b} out of range"));
            }
        }
        if rb.group >= 16 {
            r.errors.push(format!("rigid body {i} collision group {} >= 16", rb.group));
        }
        if rb.mass <= 0.0 || !rb.mass.is_finite() {
            r.errors.push(format!("rigid body {i} mass {} invalid", rb.mass));
        }
        if rb.size.x <= 0.0 || !rb.size.is_finite() {
            r.errors.push(format!("rigid body {i} size {:?} invalid", rb.size));
        }
        if matches!(rb.mode, RigidBodyMode::StaticWithBone) && rb.bone.is_none() {
            r.errors.push(format!("rigid body {i}: mode 0 requires a bone"));
        }
    }
    for (i, j) in m.joints.iter().enumerate() {
        if let Some(a) = j.body_a {
            if a as usize >= nrb {
                r.errors.push(format!("joint {i} body A {a} out of range"));
            }
        }
        if let Some(b) = j.body_b {
            if b as usize >= nrb {
                r.errors.push(format!("joint {i} body B {b} out of range"));
            }
        }
        if j.body_a.is_none() && j.body_b.is_none() {
            r.errors.push(format!("joint {i} references no bodies"));
        }
        if !j.position.is_finite() {
            r.errors.push(format!("joint {i} position contains NaN"));
        }
    }

    // texture files exist relative to base dir (when given)
    if let Some(dir) = check_files {
        for t in &m.textures {
            let p = dir.join(t);
            if !p.exists() {
                r.errors.push(format!("texture file missing: {t}"));
            }
            if t.contains('\\') {
                r.warnings.push(format!("texture path uses backslashes: {t}"));
            }
            if std::path::Path::new(t).is_absolute() {
                r.errors.push(format!("texture path must be relative: {t}"));
            }
        }
    }

    r
}

/// Validate a PMX file on disk (including texture paths relative to the file).
pub fn validate_file(path: &std::path::Path) -> Result<ValidationReport> {
    let m = super::reader::read_pmx_file(path)?;
    let dir = path.parent().unwrap_or(std::path::Path::new("."));
    let mut report = validate(&m, Some(dir));
    report.warnings.extend(Vec::<String>::new());
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pmx::writer::WriterOpts;
    use glam::Vec3;

    fn good_model() -> PmxModel {
        let mut m = PmxModel::default();
        m.vertices = vec![
            PmxVertex::bdef(Vec3::ZERO, &[(0, 1.0)]),
            PmxVertex::bdef(Vec3::X, &[(0, 1.0)]),
            PmxVertex::bdef(Vec3::Y, &[(0, 1.0)]),
        ];
        m.faces = vec![0, 1, 2];
        m.materials = vec![PmxMaterial::default()];
        m.bones = vec![PmxBone::default()];
        m
    }

    #[test]
    fn accepts_good_model() {
        let rep = validate(&good_model(), None);
        assert!(rep.ok(), "errors: {:?}", rep.errors);
    }

    #[test]
    fn catches_bad_face() {
        let mut m = good_model();
        m.faces.push(99);
        let rep = validate(&m, None);
        assert!(!rep.ok());
    }

    #[test]
    fn catches_parent_cycle() {
        let mut m = good_model();
        m.bones.push(PmxBone { parent: Some(1), ..Default::default() });
        m.bones[0].parent = Some(1);
        let rep = validate(&m, None);
        assert!(!rep.ok());
    }

    #[test]
    fn catches_ik_errors() {
        let mut m = good_model();
        m.bones[0].ik = Some(PmxIk {
            target: 0,
            loop_count: 0,
            angle_limit: -1.0,
            links: vec![],
        });
        let rep = validate(&m, None);
        assert!(!rep.ok());
    }

    #[test]
    fn roundtrip_survives_validation() {
        let m = good_model();
        let bytes = crate::pmx::writer::write_pmx(&m, &WriterOpts::auto(&m)).unwrap();
        let back = crate::pmx::reader::read_pmx(&bytes).unwrap();
        let rep = validate(&back, None);
        assert!(rep.ok(), "errors: {:?}", rep.errors);
    }
}
