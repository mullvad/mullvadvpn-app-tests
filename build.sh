#!/usr/bin/env bash

set -eu

RUSTFLAGS="-C target-feature=+crt-static" cargo build --bin test-runner --release --target "${TARGET}"

./scripts/build-runner-image.sh
