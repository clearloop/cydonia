//! Model registry code generation.
//!
//! Reads TOML files from `registry/` and generates Rust source files into
//! `OUT_DIR`. Called from `build.rs`.

mod emit;
mod parse;

/// Run the code generation.
///
/// Reads `CARGO_FEATURE_METAL` / `CARGO_FEATURE_CUDA` to select the
/// platform, then generates `quantization.rs` and `registry.rs`.
pub fn run() {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let registry_dir = std::path::Path::new("registry");

    // Select platform based on cargo features.
    let platform_name = if std::env::var("CARGO_FEATURE_METAL").is_ok() {
        "metal"
    } else if std::env::var("CARGO_FEATURE_CUDA").is_ok() {
        "cuda"
    } else {
        "cpu"
    };

    let platform_path = registry_dir.join(format!("{platform_name}.toml"));
    let models_path = registry_dir.join("models.toml");

    let platform = parse::load_platform(&platform_path);
    let models = parse::load_models(&models_path);

    emit::write_quantization(&platform, &out_dir);
    emit::write_registry(&platform, &models, platform_name, &out_dir);

    // Rerun if any registry file changes.
    println!("cargo::rerun-if-changed=registry/metal.toml");
    println!("cargo::rerun-if-changed=registry/cuda.toml");
    println!("cargo::rerun-if-changed=registry/cpu.toml");
    println!("cargo::rerun-if-changed=registry/models.toml");
}
