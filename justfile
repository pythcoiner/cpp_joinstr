clean:
    rm -fRd target
    rm -fRd include
    rm Cargo.lock

build:
    just clean
    cargo build --release
