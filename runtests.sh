#!/usr/bin/env bash

set -eu

export TARGET=${TARGET:-"x86_64-unknown-linux-gnu"}

serial_port=serial_port_linux

case $TARGET in
    "x86_64-unknown-linux-gnu")
        serial_port=serial_port_linux
        ;;

    "x86_64-pc-windows-gnu")
        serial_port=serial_port_windows
        ;;

    *-darwin)
        echo "Not yet supported"
        exit 1
        ;;

    *)
        echo "Unknown target: $TARGET"
        exit 1
        ;;
esac

if [[ -n ${1+x} ]]; then
    case $1 in
        "linux")
            serial_port=serial_port_linux
            shift
            ;;

        "windows")
            serial_port=serial_port_windows
            shift
            ;;
        "mac")
            echo "Mac is not yet supported"
            exit 1
            ;;
        *);;
    esac
fi

cargo build -p test-manager
sudo RUST_LOG=debug ACCOUNT_TOKEN=$ACCOUNT_TOKEN ./target/debug/test-manager $serial_port $@
