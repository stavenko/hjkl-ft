//! Build step: convert the Story DSL from YAML (authored) to JSON (shipped).
//!
//! The story is authored in `story/story.yaml` for readable multiline prose, but
//! we don't want a YAML parser in the wasm bundle. So at build time we parse the
//! YAML and re-emit it as `story.json` in `OUT_DIR`; the crate embeds that JSON
//! via `include_str!` and parses it at runtime with `serde_json` (already a dep).
//! YAML errors fail the build loudly here, not at runtime.

use std::path::Path;

fn main() {
    let yaml_path = "story/story.yaml";
    println!("cargo:rerun-if-changed={yaml_path}");

    let yaml = std::fs::read_to_string(yaml_path)
        .unwrap_or_else(|e| panic!("build.rs: cannot read {yaml_path}: {e}"));

    // Round-trip through an untyped value: validates YAML syntax and produces
    // JSON. Type-level validation happens when the crate deserializes into the
    // Story structs (so a schema mistake still fails the wasm build).
    let value: serde_yaml::Value = serde_yaml::from_str(&yaml)
        .unwrap_or_else(|e| panic!("build.rs: invalid YAML in {yaml_path}: {e}"));
    let json = serde_json::to_string(&value)
        .unwrap_or_else(|e| panic!("build.rs: YAML→JSON failed: {e}"));

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
    let out_path = Path::new(&out_dir).join("story.json");
    std::fs::write(&out_path, json)
        .unwrap_or_else(|e| panic!("build.rs: cannot write {}: {e}", out_path.display()));
}
