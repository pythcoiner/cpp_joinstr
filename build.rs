use std::fs;

fn main() {
    cxx_build::bridge("src/lib.rs")
        .flag_if_supported("-std=c++17")
        .compile("cpp_joinstr");

    // Manually list all .rs files under src/
    for entry in fs::read_dir("src").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }
}
