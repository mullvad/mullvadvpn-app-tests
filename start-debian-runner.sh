#!/usr/bin/env bash

set -eu

TEMP_IMAGE=$(mktemp)

TARGET="x86_64-unknown-linux-gnu" ./build.sh

cp ./qemu-images/debian.img "${TEMP_IMAGE}"

qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -drive file="${TEMP_IMAGE}" \
    -drive file=./qemu-images/test-runner.img \
    -device virtio-serial-pci -serial pty \
    -nographic

rm "${TEMP_IMAGE}"
