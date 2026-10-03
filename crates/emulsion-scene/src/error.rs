//! Errors.

use thiserror::Error;

/// Everything that can go wrong building, importing or validating a scene.
/// Bad input never panics; it ends up here.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SceneError {
    /// The scene or a parameter breaks a rule.
    #[error("invalid scene: {0}")]
    Invalid(String),
    /// A size limit was exceeded.
    #[error("limit exceeded: {0}")]
    Limit(String),
    /// JSON could not be parsed.
    #[error("parse error: {0}")]
    Parse(String),
    /// A model file could not be imported.
    #[error("import failed: {0}")]
    Import(String),
    /// An object id or asset is missing.
    #[error("not found: {0}")]
    NotFound(String),
}
