//! TOML parsing for the model registry codegen.

use serde::Deserialize;
use std::{collections::BTreeMap, fs, path::Path};

/// Registry configuration parsed from `models.toml`.
#[derive(Debug, Deserialize)]
pub struct RegistryFile {
    pub defaults: RegistryDefaults,
    pub models: BTreeMap<String, FamilyEntry>,
}

/// Default model specifier (e.g. `"qwen3-vl:4b"`).
#[derive(Debug, Deserialize)]
pub struct RegistryDefaults {
    pub text: String,
}

/// A model family with a shared loader and multiple tags.
#[derive(Debug, Deserialize)]
pub struct FamilyEntry {
    pub loader: String,
    pub tags: Vec<String>,
}

/// Load and parse the registry TOML file.
pub fn load_registry(path: &Path) -> RegistryFile {
    let content = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    toml::from_str(&content).unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()))
}
