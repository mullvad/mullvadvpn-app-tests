#!/usr/bin/env bash

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

if [[ $TARGET == x86_64-unknown-linux-gnu ]]; then
    mkdir -p .container/cargo-registry
    podman build -t mullvadvpn-app-tests .

    podman run \
        -v "$PWD/.container/cargo-registry":/root/.cargo/registry \
        -v "$PWD":/src:Z \
        -e CARGO_HOME=/root/.cargo/registry \
        mullvadvpn-app-tests \
        /bin/bash -c "cd /src/; cargo build --bin test-runner --release --target ${TARGET}"
else
    cargo build --bin test-runner --release --target "${TARGET}"
fi

./scripts/build-runner-image.sh
