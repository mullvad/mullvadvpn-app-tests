#!/usr/bin/env bash

# This script produces a virtual disk containing the test runner binaries.
# The resulting disk, ../qemu-images/windows-test-runner.img, should be mounted
# to E:\ in the guest.

set -eu

HARNESS_SIZE_MB=500
HARNESS_IMAGE=windows-test-runner.img

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

echo "Creating empty disk image for test runner"

IMG_PATH="${SCRIPT_DIR}/../qemu-images/${HARNESS_IMAGE}"

dd if=/dev/null of="${IMG_PATH}" bs=1M seek="${HARNESS_SIZE_MB}"

echo "Copying files to image"

mformat -i "${IMG_PATH}" "::"

mcopy \
    -i "${IMG_PATH}" \
    "${SCRIPT_DIR}/../target/x86_64-pc-windows-gnu/release/test-runner.exe" \
    "::"

mdir -i "${IMG_PATH}"

echo "Success!"
