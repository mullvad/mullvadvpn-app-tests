set -eu

RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target "${TARGET}"

./scripts/build-runner-image.sh
