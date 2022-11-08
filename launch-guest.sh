#!/usr/bin/env bash

set -eu

export TARGET=${TARGET:-"x86_64-unknown-linux-gnu"}

./build.sh

case $TARGET in

    "x86_64-unknown-linux-gnu")
        SERIAL_PORT=serial_port_linux
        OSIMAGE=./os-images/debian.qcow2
        RUNNERIMAGE=./testrunner-images/linux-test-runner.img
        ;;

    "x86_64-pc-windows-gnu")
        SERIAL_PORT=serial_port_windows
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

# Check if we need to setup the network
ip link show br-mullvadtest >&/dev/null || sudo ./scripts/setup-network.sh

pty=$(python3 -<<END_SCRIPT
import os
master, slave = os.openpty()
print(os.ttyname(slave))
END_SCRIPT
)

trap "rm -f $SERIAL_PORT" EXIT
ln -s $pty $SERIAL_PORT

sudo qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -snapshot \
    -drive file="${OSIMAGE}" \
    -drive if=none,id=runner,file="${RUNNERIMAGE}" \
    -device nec-usb-xhci,id=xhci \
    -device usb-storage,drive=runner,bus=xhci.0 \
    -device virtio-serial-pci -serial pty \
    -nic tap,ifname=tap-mullvadtest,script=no,downscript=no
