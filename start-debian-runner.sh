#!/usr/bin/env bash

set -eu

TARGET="x86_64-unknown-linux-gnu" ./build.sh

qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -snapshot \
    -drive file=./qemu-images/debian.qcow2 \
    -drive file=./qemu-images/test-runner.img \
    -device virtio-serial-pci -serial pty \
    -nographic
