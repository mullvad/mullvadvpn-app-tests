#!/usr/bin/env bash

# This script produces a virtual disk containing the test runner binaries.
# The resulting disk, ../qemu-images/{OS}-test-runner.img, should be mounted to:
# * /opt/testing for Linux guests.
# * E: for Windows guests.

set -eu

HARNESS_SIZE_MB=500

case $TARGET in
    "x86_64-unknown-linux-gnu")
        HARNESS_IMAGE=linux-test-runner.img
        ;;
    "x86_64-pc-windows-gnu")
        HARNESS_IMAGE=windows-test-runner.img
        ;;
    *)
        echo "Unknown target: $TARGET"
        exit 1
        ;;
esac

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

echo "**********************************"
echo "* Creating empty disk image"
echo "**********************************"

IMG_PATH="${SCRIPT_DIR}/../qemu-images/${HARNESS_IMAGE}"
dd if=/dev/null of="${IMG_PATH}" bs=1M seek="${HARNESS_SIZE_MB}"

echo "**********************************"
echo "* Preparing test runner image"
echo "**********************************"

case $TARGET in

    "x86_64-unknown-linux-gnu")
        mkfs.ext4 -F "${IMG_PATH}"
        e2cp \
            -P 500 \
            "${SCRIPT_DIR}/../target/x86_64-unknown-linux-gnu/release/test-runner" \
            "${IMG_PATH}:/"
        e2cp \
            "${SCRIPT_DIR}/../packages/"*.deb \
            "${IMG_PATH}:/"
        ;;

    "x86_64-pc-windows-gnu")
        mformat -i "${IMG_PATH}" "::"
        mcopy \
            -i "${IMG_PATH}" \
            "${SCRIPT_DIR}/../target/x86_64-pc-windows-gnu/release/test-runner.exe" \
            "${SCRIPT_DIR}/../packages/"*.exe \
            "::"
        mdir -i "${IMG_PATH}"
        ;;

esac

echo "**********************************"
echo "* Success!"
echo "**********************************"
