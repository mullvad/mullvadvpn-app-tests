TARGET=${TARGET:-"x86_64-unknown-linux-gnu"}

RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target "${TARGET}"

./scripts/build-runner-image.sh
