//! Model registry code generation.
//!
//! Reads `registry/models.toml` and generates Rust source files into `OUT_DIR`.
//! Called from `build.rs`.

mod emit;
mod parse;

/// Run the code generation.
pub fn run() {
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let registry_path = std::path::Path::new("registry/models.toml");
    let registry = parse::load_registry(registry_path);

    emit::write_memory_threshold(&out_dir);
    emit::write_registry(&registry, &out_dir);

    println!("cargo::rerun-if-changed={}", registry_path.display());
}
