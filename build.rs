use cxx_qt_build::CxxQtBuilder;

fn main() {
    CxxQtBuilder::new()
        .file("src/lib.rs")
        // .file("src/coin.rs")
        // .file("src/pool_config.rs")
        .build();
}
