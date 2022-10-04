#!/usr/bin/env bash

set -eu

export TARGET=${TARGET:-"x86_64-unknown-linux-gnu"}

./build.sh

case $TARGET in

    "x86_64-unknown-linux-gnu")
        OSIMAGE=./os-images/debian.qcow2
        RUNNERIMAGE=./testrunner-images/linux-test-runner.img
        ;;

    "x86_64-pc-windows-gnu")
        OSIMAGE=./os-images/windows10.qcow2
        RUNNERIMAGE=./testrunner-images/windows-test-runner.img
        ;;

    *-darwin)
        # NOTE: QEMU does not yet support M1; must use
        # virtualization framework for that.
        # We're severely limited by UTM.
        open "utm://start?name=mullvad-macOS"
        exit 0

        ;;

    *)
        echo "Unknown target: $TARGET"
        exit 1
        ;;

esac

sudo ./scripts/setup-network.sh

sudo qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -snapshot \
    -drive file="${OSIMAGE}" \
    -drive file="${RUNNERIMAGE}" \
    -device virtio-serial-pci -serial pty \
    -nic tap,ifname=tap-mullvadtest,script=no,downscript=no

