#!/usr/bin/env bash

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

cargo build --bin test-runner --release --target "${TARGET}"

./scripts/build-runner-image.sh
