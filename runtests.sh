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

LAUNCH_ONLY=${LAUNCH_ONLY:-""}

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
        ;;
    *)
        echo "Unknown target: $TARGET"
        exit 1
        ;;

esac

./build.sh

echo "Compiling tests"
cargo build -p test-manager

function run_tests {
    local pty=$1
    shift

    echo "Executing tests"

    sleep 1
    RUST_LOG=debug ACCOUNT_TOKEN=${ACCOUNT_TOKEN} HOST_NET_INTERFACE=${HOST_NET_INTERFACE} ./target/debug/test-manager ${pty} $@
}

function trap_handler {
    if [[ -n "${QEMU_PID+x}" ]]; then
        kill --timeout 5000 KILL -TERM -- $QEMU_PID >/dev/null 2>&1 || true
    fi

    if [[ $TARGET == *-darwin ]]; then
        if [[ -z ${LAUNCH_ONLY} ]]; then
            open "utm://stop?name=mullvad-macOS"
        fi
    fi

    if [[ -n "${HOST_NET_INTERFACE+x}" ]] &&
          ip link show "${HOST_NET_INTERFACE}" >&/dev/null; then
        echo "Removing interface ${HOST_NET_INTERFACE}"
        ip link del dev ${HOST_NET_INTERFACE}
    fi
}

trap "trap_handler" EXIT TERM

pty=$(python3 -<<END_SCRIPT
import os
master, slave = os.openpty()
print(os.ttyname(slave))
END_SCRIPT
)

if [[ $TARGET == *-darwin ]]; then
    # NOTE: QEMU does not yet support M1; must use
    # virtualization framework for that.
    # We're severely limited by UTM.
    open "utm://start?name=mullvad-macOS"
    HOST_NET_INTERFACE=placeholder
    run_tests ${pty} $@
    exit 0
fi

# Check if we need to setup the network
ip link show br-mullvadtest >&/dev/null || ./scripts/setup-network.sh

HOST_NET_INTERFACE=tap-mullvad$(cat /dev/urandom | tr -dc 'a-z' | head -c 4)

echo "Creating network interface $HOST_NET_INTERFACE"

ip tuntap add ${HOST_NET_INTERFACE} mode tap
ip link set ${HOST_NET_INTERFACE} master br-mullvadtest
ip link set ${HOST_NET_INTERFACE} up

echo "Launching guest VM"

qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
    -snapshot \
    -drive file="${OSIMAGE}" \
    -drive if=none,id=runner,file="${RUNNERIMAGE}" \
    -device nec-usb-xhci,id=xhci \
    -device usb-storage,drive=runner,bus=xhci.0 \
    -device virtio-serial-pci -serial pty \
    ${DISPLAY_ARG} \
    -nic tap,ifname=${HOST_NET_INTERFACE},script=no,downscript=no &

QEMU_PID=$!

if [[ -n ${LAUNCH_ONLY} ]]; then
    wait -f $QEMU_PID
    exit 0
fi

run_tests ${pty} $@
