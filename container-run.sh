#!/usr/bin/env bash

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

mkdir -p .container-cache/.cargo/{target,registry,git}

podman run --rm -it \
    --cap-add=NET_ADMIN --cap-add=NET_RAW \
    --security-opt="label=disable" \
    --device=/dev/net/tun \
    --device=/dev/kvm \
    --sysctl net.ipv4.ip_forward=1 \
    -e="OS=$OS" \
    -e="SHOW_DISPLAY=${SHOW_DISPLAY:-""}" \
    -e="ACCOUNT_TOKEN=$ACCOUNT_TOKEN" \
    -e="CURRENT_APP_FILENAME=$CURRENT_APP_FILENAME" \
    -e="PREVIOUS_APP_FILENAME=$PREVIOUS_APP_FILENAME" \
    -v "$SCRIPT_DIR:/build:Z" \
    -v "$SCRIPT_DIR/.container-cache/.cargo/target:/root/.cargo/target:Z" \
    -v "$SCRIPT_DIR/.container-cache/.cargo/registry:/root/.cargo/registry:Z" \
    -v "$SCRIPT_DIR/.container-cache/.cargo/git:/root/.cargo/git:Z" \
    mullvadvpn-app-tests \
    bash -c "$*"
