use std::fs;

use cxx_qt_build::CxxQtBuilder;

fn main() {
    CxxQtBuilder::new()
        .file("src/lib.rs")
        .build();

    // Manually list all .rs files under src/
    for entry in fs::read_dir("src").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            println!("cargo::rerun-if-changed={}", path.display());
        }
    }
}
