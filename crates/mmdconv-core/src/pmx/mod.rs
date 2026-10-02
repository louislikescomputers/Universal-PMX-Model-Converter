//! PMX 2.0/2.1 support: data model, binary reader, binary writer, validator.

pub mod model;
pub mod reader;
pub mod validate;
pub mod writer;

pub use model::*;
pub use reader::{read_pmx, read_pmx_file};
pub use validate::{validate, validate_file, ValidationReport};
pub use writer::{write_pmx, write_pmx_file, WriterOpts};

/// Recompute the smallest legal index sizes for a parsed model (used by the
/// PMX→PMX cleanup path).
pub fn recompute_index_sizes(m: &mut PmxModel) {
    m.vertex_size = IdSize::for_count(m.vertices.len().max(1));
    m.texture_size = IdSize::for_count(m.textures.len());
    m.material_size = IdSize::for_count(m.materials.len());
    m.bone_size = IdSize::for_count(m.bones.len().max(1));
    m.morph_size = IdSize::for_count(m.morphs.len());
    m.rigid_body_size = IdSize::for_count(m.rigid_bodies.len());
}
