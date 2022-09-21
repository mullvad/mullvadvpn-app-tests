#!/usr/bin/env bash

# This script produces a virtual disk containing the test runner binaries.
# The resulting disk, ../qemu-images/test-runner.img, should be mounted to
# /opt/testing in the guest.

set -eu

HARNESS_SIZE_MB=500
HARNESS_IMAGE=test-runner.img
HARNESS_MOUNT_POINT=/tmp/harness

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

function cleanup {
    umount "${HARNESS_MOUNT_POINT}"
}

echo "Creating empty disk image for test harness"

IMG_PATH="${SCRIPT_DIR}/../qemu-images/${HARNESS_IMAGE}"

dd if=/dev/null of=${IMG_PATH} bs=1M seek="${HARNESS_SIZE_MB}"
mkfs.ext4 -F "${IMG_PATH}"

mkdir -p "${HARNESS_MOUNT_POINT}"
mount -t ext4 -o loop "${IMG_PATH}" "${HARNESS_MOUNT_POINT}"

trap "cleanup" EXIT

echo "Copying files to image"

cp "${SCRIPT_DIR}/../target/x86_64-unknown-linux-gnu/release/test-tarpc" "${HARNESS_MOUNT_POINT}"
cp "${SCRIPT_DIR}/../packages/"*.deb "${HARNESS_MOUNT_POINT}"
