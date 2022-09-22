#!/usr/bin/env bash

set -eu

TARGET="x86_64-pc-windows-gnu" ./build.sh

qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -snapshot \
    -drive file=./qemu-images/windows10.qcow2 \
    -drive file=./qemu-images/windows-test-runner.img \
    -device virtio-serial-pci -serial pty
