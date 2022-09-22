#!/usr/bin/env bash

set -eu

export TARGET=${TARGET:-"x86_64-unknown-linux-gnu"}

./build.sh

case $TARGET in

    "x86_64-unknown-linux-gnu")
        OSIMAGE=./qemu-images/debian.qcow2
        RUNNERIMAGE=./qemu-images/linux-test-runner.img
        ;;

    "x86_64-pc-windows-gnu")
        OSIMAGE=./qemu-images/windows10.qcow2
        RUNNERIMAGE=./qemu-images/windows-test-runner.img
        ;;

    *)
        echo "Unknown target: $TARGET"
        exit 1
        ;;

esac

qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -snapshot \
    -drive file="${OSIMAGE}" \
    -drive file="${RUNNERIMAGE}" \
    -device virtio-serial-pci -serial pty
