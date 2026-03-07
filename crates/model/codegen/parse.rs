//! TOML parsing for the model registry codegen.

use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

/// Platform configuration parsed from `{platform}.toml`.
#[derive(Debug, Deserialize)]
pub struct PlatformFile {
    pub defaults: PlatformDefaults,
    pub quantizations: BTreeMap<String, String>,
}

/// Default model keys and quantization for a platform.
#[derive(Debug, Deserialize)]
pub struct PlatformDefaults {
    pub text: String,
    pub vision: Option<String>,
    pub quantization: Option<String>,
}

/// Top-level models file: map of model key → model definition.
pub type ModelsFile = BTreeMap<String, ModelDef>;

/// A single model definition from `models.toml`.
#[derive(Debug, Deserialize)]
pub struct ModelDef {
    pub name: String,
    pub memory: String,
    pub metal: Option<PlatformVariant>,
    pub cuda: Option<PlatformVariant>,
    pub cpu: Option<PlatformVariant>,
}

/// Platform-specific variant of a model.
#[derive(Debug, Deserialize)]
pub struct PlatformVariant {
    pub model_id: String,
    pub loader: String,
}

/// Load and parse a platform TOML file.
pub fn load_platform(path: &Path) -> PlatformFile {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}

/// Load and parse the models TOML file.
pub fn load_models(path: &Path) -> ModelsFile {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}
