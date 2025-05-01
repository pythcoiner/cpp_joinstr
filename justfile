clean:
    rm -fRd target
    rm -fRd include
    rm -f Cargo.lock

build:
    just clean
    cargo build --release
