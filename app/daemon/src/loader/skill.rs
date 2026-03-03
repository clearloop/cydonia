//! Skill markdown loading — re-exports from the skill crate.
//!
//! The actual parsing and loading logic lives in `walrus-skill::loader`.
//! This module re-exports the public API for backward compatibility with
//! daemon code that references `crate::loader::load_skills_dir`.

pub use skill::loader::{load_skills_dir, parse_skill_md};
