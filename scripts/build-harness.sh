#!/usr/bin/env bash

# This script produces a virtual disk out of this repository.
# The resultant disk should be mounted to /opt/testing

set -eu

HARNESS_SIZE_MB=100
HARNESS_IMAGE=harness.img
HARNESS_MOUNT_POINT=/tmp/harness

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
cd "${SCRIPT_DIR}"

echo "Creating empty disk image for test harness"

dd if=/dev/null of=harness.img bs=1M seek="${HARNESS_SIZE_MB}"
mkfs.ext4 -F "${HARNESS_IMAGE}"

echo "Cloning repository to image"

mkdir -p "${HARNESS_MOUNT_POINT}"
mount -t ext4 -o loop "${HARNESS_IMAGE}" "${HARNESS_MOUNT_POINT}"

cd "${HARNESS_MOUNT_POINT}"
git clone "$( dirname ${SCRIPT_DIR} )"

cd "${SCRIPT_DIR}"

umount "${HARNESS_MOUNT_POINT}"
