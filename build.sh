export CXX_QT_EXPORT_CRATE_qt_joinstr=1
export CXX_QT_EXPORT_DIR="./include"
cargo build --release
# mv ./include/crates/qt_joinstr/include/qt_joinstr/src/lib.cxx.h ./include/qt_joinstr.h
# rm -fRd ./include/crates
