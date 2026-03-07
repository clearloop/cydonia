//! TOML parsing for the model registry codegen.

use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

/// Platform configuration parsed from `{platform}.toml`.
///
/// Each platform TOML is self-contained: defaults and the full model
/// list for that platform.
#[derive(Debug, Deserialize)]
pub struct PlatformFile {
    pub defaults: PlatformDefaults,
    pub models: BTreeMap<String, ModelEntry>,
}

/// Default model key for a platform.
#[derive(Debug, Deserialize)]
pub struct PlatformDefaults {
    pub text: String,
}

/// A single model entry within a platform TOML.
#[derive(Debug, Deserialize)]
pub struct ModelEntry {
    pub name: String,
    pub memory: String,
    pub model_id: String,
    pub loader: String,
    pub gguf_stem: Option<String>,
    pub gguf_file: Option<String>,
}

/// Load and parse a platform TOML file.
pub fn load_platform(path: &Path) -> PlatformFile {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}
