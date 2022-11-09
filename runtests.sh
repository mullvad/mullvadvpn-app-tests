#!/usr/bin/env bash

set -eu

export TARGET=${TARGET:-"x86_64-unknown-linux-gnu"}

if [[ -z "${ACCOUNT_TOKEN+x}" ]]; then
    echo "'ACCOUNT_TOKEN' must be specified"
    exit 1
fi

if [[ -z "${SHOW_DISPLAY+x}" ]]; then
    DISPLAY_ARG="-display none"
else
    DISPLAY_ARG=""
fi

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

# Check if we need to setup the network
ip link show br-mullvadtest >&/dev/null || sudo ./scripts/setup-network.sh

./build.sh

echo "Compiling tests"

cargo build -p test-manager

echo "Launching guest VM"

sudo echo

pty=$(python3 -<<END_SCRIPT
import os
master, slave = os.openpty()
print(os.ttyname(slave))
END_SCRIPT
)

sudo qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -snapshot \
    -drive file="${OSIMAGE}" \
    -drive if=none,id=runner,file="${RUNNERIMAGE}" \
    -device nec-usb-xhci,id=xhci \
    -device usb-storage,drive=runner,bus=xhci.0 \
    -device virtio-serial-pci -serial pty \
    ${DISPLAY_ARG} \
    -nic tap,ifname=tap-mullvadtest,script=no,downscript=no &

QEMU_PID=$!

trap "sudo kill -KILL -- $QEMU_PID >/dev/null 2>&1 || true" EXIT TERM

echo "Executing tests"

sleep 1
sudo RUST_LOG=debug ACCOUNT_TOKEN=$ACCOUNT_TOKEN ./target/debug/test-manager ${pty} $@

sudo kill --timeout 5000 KILL -TERM -- $QEMU_PID >/dev/null 2>&1 || true
