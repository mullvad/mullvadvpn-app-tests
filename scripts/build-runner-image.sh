#!/usr/bin/env bash

# This script produces a virtual disk containing the test runner binaries.
# The resulting disk, ../qemu-images/test-runner.img, should be mounted to
# /opt/testing in the guest.

set -eu

HARNESS_SIZE_MB=500
HARNESS_IMAGE=test-runner.img

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

echo "Creating empty disk image for test runner"

IMG_PATH="${SCRIPT_DIR}/../qemu-images/${HARNESS_IMAGE}"

dd if=/dev/null of="${IMG_PATH}" bs=1M seek="${HARNESS_SIZE_MB}"
mkfs.ext4 -F "${IMG_PATH}"

echo "Copying files to image"

e2cp \
    -P 500 \
    "${SCRIPT_DIR}/../target/x86_64-unknown-linux-gnu/release/test-runner" \
    "${IMG_PATH}:/"

e2cp \
    "${SCRIPT_DIR}/../packages/"*.deb \
    "${IMG_PATH}:/"

echo "Success!"
