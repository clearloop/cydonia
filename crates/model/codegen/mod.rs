//! Model registry code generation.
//!
//! Reads a single platform TOML from `registry/` and generates Rust source
//! files into `OUT_DIR`. Called from `build.rs`.

mod emit;
mod parse;

/// Run the code generation.
///
/// Reads `CARGO_FEATURE_METAL` / `CARGO_FEATURE_CUDA` to select the
/// platform, then generates `MemoryThreshold` and `registry.rs`.
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
    let platform = parse::load_platform(&platform_path);

    emit::write_memory_threshold(&out_dir);
    emit::write_registry(&platform, &out_dir);

    println!("cargo::rerun-if-changed={}", platform_path.display());
}
