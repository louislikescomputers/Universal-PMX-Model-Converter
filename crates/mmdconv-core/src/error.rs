//! Error types for the mmdconv pipeline.

use std::path::PathBuf;

/// Result alias used throughout the crate.
pub type Result<T> = std::result::Result<T, MmdconvError>;

#[derive(Debug, thiserror::Error)]
pub enum MmdconvError {
    #[error("I/O error: {ctx}\n  {source}")]
    Io { ctx: String, source: std::io::Error },

    #[error("{0}")]
    Input(String),

    #[error("unsupported format: {0}")]
    Unsupported(String),

    #[error("PMX parse error at byte {offset}: {message}")]
    PmxParse { offset: usize, message: String },

    #[error("validation failed with {} problem(s):\n{}", .problems.len(), .problems.iter().map(|p| format!("  - {p}")).collect::<Vec<_>>().join("\n"))]
    Validation { problems: Vec<String> },
}

impl MmdconvError {
    pub fn io(ctx: impl Into<String>, source: std::io::Error) -> Self {
        MmdconvError::Io { ctx: ctx.into(), source }
    }
    pub fn input(msg: impl Into<String>) -> Self {
        MmdconvError::Input(msg.into())
    }
    pub fn pmx_parse(offset: usize, msg: impl Into<String>) -> Self {
        MmdconvError::PmxParse { offset, message: msg.into() }
    }
}

/// Human-readable error with fix hints (used by the CLI).
pub fn describe_with_hint(e: &MmdconvError, path: Option<&std::path::Path>) -> String {
    let mut s = format!("error: {e}");
    let hint = match e {
        MmdconvError::Io { .. } => match path {
            Some(p) if !p.exists() => "hint: check that the file exists and the path is spelled correctly (watch for smart quotes or full-width characters in Japanese filenames)".to_string(),
            _ => "hint: check file permissions and available disk space".to_string(),
        },
        MmdconvError::Unsupported(m) => format!("hint: supported inputs are .glb/.gltf/.vrm/.fbx/.dae/.pmx/.pmd/.obj/.stl/.ply; for other formats convert to glTF first (detected: {m})"),
        MmdconvError::Input(_) => "hint: run `mmdconv inspect <file>` to see what the importer read".to_string(),
        MmdconvError::PmxParse { .. } => "hint: the file may be truncated or corrupt; try re-exporting it from PMXEditor".to_string(),
        MmdconvError::Validation { .. } => "hint: report this as a bug if it happened on a freshly generated file".to_string(),
    };
    s.push('\n');
    s.push_str(&hint);
    s
}

pub fn path_ctx(p: &PathBuf) -> String {
    p.display().to_string()
}
