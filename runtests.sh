#!/usr/bin/env bash

set -eu

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "$SCRIPT_DIR"

case ${OS} in

    "debian11")
        export TARGET="x86_64-unknown-linux-gnu"
        OSIMAGE=./os-images/debian11.qcow2
        RUNNERIMAGE=./testrunner-images/linux-test-runner.img
        ;;

    "ubuntu2204")
        export TARGET="x86_64-unknown-linux-gnu"
        OSIMAGE=./os-images/ubuntu2204.qcow2
        RUNNERIMAGE=./testrunner-images/linux-test-runner.img
        ;;

    "fedora37")
        export TARGET="x86_64-unknown-linux-gnu"
        OSIMAGE=./os-images/fedora37.qcow2
        RUNNERIMAGE=./testrunner-images/linux-test-runner.img
        ;;

    "windows10")
        export TARGET="x86_64-pc-windows-gnu"
        OSIMAGE=./os-images/windows10.qcow2
        RUNNERIMAGE=./testrunner-images/windows-test-runner.img
        ;;

    "windows11")
        export TARGET="x86_64-pc-windows-gnu"
        OSIMAGE=./os-images/windows11.qcow2
        RUNNERIMAGE=./testrunner-images/windows-test-runner.img
        ;;

    "macos")
        export TARGET=aarch64-apple-darwin
        ;;

    *)
        echo "Unknown OS: $OS"
        exit 1
        ;;

esac

if [[ -z "${ACCOUNT_TOKEN+x}" ]]; then
    echo "'ACCOUNT_TOKEN' must be specified"
    exit 1
fi

if [[ -z "${SHOW_DISPLAY+x}" ]]; then
    DISPLAY_ARG="-display none"
else
    DISPLAY_ARG=""
fi

if [[ "$TARGET" != *-darwin && "$EUID" -ne 0 ]]; then
    echo "Using rootlesskit since uid != 0"
    rootlesskit --net slirp4netns --disable-host-loopback --copy-up=/etc "${BASH_SOURCE[0]}" "$@"
    exit 0
fi

if [[ -z "${SKIP_COMPILATION+x}" ]]; then
    ./build.sh

    echo "Compiling tests"
    cargo build -p test-manager
fi

function run_tests {
    local pty=$1
    shift

    echo "Executing tests"

    sleep 1
    RUST_LOG=debug ACCOUNT_TOKEN=${ACCOUNT_TOKEN} HOST_NET_INTERFACE=${HOST_NET_INTERFACE} ./target/debug/test-manager ${pty} $@
}

function trap_handler {
    if [[ -n "${QEMU_PID+x}" ]]; then
        env kill --timeout 5000 KILL -TERM -- $QEMU_PID >/dev/null 2>&1 || true
    fi

    if [[ $TARGET == *-darwin ]]; then
        if [[ -z ${SHOW_DISPLAY+x} ]]; then
            open "utm://stop?name=mullvad-macOS"
        fi
    fi

    if [[ -n "${HOST_NET_INTERFACE+x}" ]] &&
          ip link show "${HOST_NET_INTERFACE}" >&/dev/null; then
        echo "Removing interface ${HOST_NET_INTERFACE}"
        ip link del dev ${HOST_NET_INTERFACE}
    fi

    if [[ -n ${DNSMASQ_PID+x} ]]; then
        echo "Killing dnsmasq: ${DNSMASQ_PID}"
        env kill -- ${DNSMASQ_PID}
    fi

    if [[ -n ${TPM_PID+x} ]]; then
        env kill -- ${TPM_PID}
    fi
}

trap "trap_handler" EXIT TERM

if [[ ${OS} == "windows11" ]]; then
    # Windows 11 requires a TPM
    tpm_dir=$(mktemp -d)
    swtpm socket -t --ctrl type=unixio,path="$tpm_dir/tpmsock"  --tpmstate dir="$tpm_dir" --tpm2 &
    TPM_PID=$!
    TPM_ARGS="-tpmdev emulator,id=tpm0,chardev=chrtpm -chardev socket,id=chrtpm,path="$tpm_dir/tpmsock" -device tpm-tis,tpmdev=tpm0"

    # Secure boot is also required
    # So we need UEFI/OVMF
    OVMF_VARS_FILENAME="OVMF_VARS.secboot.fd"
    OVMF_VARS="$SCRIPT_DIR/$OVMF_VARS_FILENAME"
    OVMF_CODE="/usr/share/OVMF/OVMF_CODE.secboot.fd"
    if [[ ! -e $OVMF_VARS ]]; then
        cp "/usr/share/OVMF/$OVMF_VARS_FILENAME" $OVMF_VARS
    fi
    OVMF_ARGS="-global driver=cfi.pflash01,property=secure,value=on \
-drive if=pflash,format=raw,unit=0,file=${OVMF_CODE},readonly=on \
-drive if=pflash,format=raw,unit=1,file=${OVMF_VARS}"

    # Q35 supports secure boot
    MACHINE_ARGS="-machine q35,smm=on"
fi

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

DNSMASQ_PID=$(cat "${SCRIPT_DIR}/scripts/.dnsmasq.pid")
HOST_NET_INTERFACE=tap-mullvad$(cat /dev/urandom | tr -dc 'a-z' | head -c 4)

echo "Creating network interface $HOST_NET_INTERFACE"

ip tuntap add ${HOST_NET_INTERFACE} mode tap
ip link set ${HOST_NET_INTERFACE} master br-mullvadtest
ip link set ${HOST_NET_INTERFACE} up

echo "Launching guest VM"

qemu-system-x86_64 -cpu host -accel kvm -m 4096 -smp 2 \
    -snapshot \
    -drive file="${OSIMAGE}" \
    -drive if=none,id=runner,file="${RUNNERIMAGE}" \
    -device nec-usb-xhci,id=xhci \
    -device usb-storage,drive=runner,bus=xhci.0 \
    -device virtio-serial-pci -serial pty \
    ${DISPLAY_ARG} \
    ${TPM_ARGS:-""} \
    ${OVMF_ARGS:-""} \
    ${MACHINE_ARGS:-""} \
    -nic tap,ifname=${HOST_NET_INTERFACE},script=no,downscript=no &

QEMU_PID=$!

if run_tests ${pty} $@; then
    EXIT_STATUS=0
else
    EXIT_STATUS=$?
fi

if [[ -n ${SHOW_DISPLAY+x} ]]; then
    wait -f $QEMU_PID
fi

exit $EXIT_STATUS
