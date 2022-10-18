#!/usr/bin/env bash

# This script produces a virtual disk containing the test runner binaries.
# The resulting disk, ../testrunner-images/{OS}-test-runner.img, should be
# mounted to:
# * /opt/testing for Linux guests.
# * E: for Windows guests.
# * /Volumes/testing for macOS guests.

set -eu

HARNESS_SIZE_MB=500

case $TARGET in
    "x86_64-unknown-linux-gnu")
        HARNESS_IMAGE=linux-test-runner.img
        ;;
    "x86_64-pc-windows-gnu")
        HARNESS_IMAGE=windows-test-runner.img
        ;;
    *-darwin)
        HARNESS_IMAGE=macos-test-runner.dmg
        ;;
    *)
        echo "Unknown target: $TARGET"
        exit 1
        ;;
esac

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

mkdir -p "${SCRIPT_DIR}/../testrunner-images/"
IMG_PATH="${SCRIPT_DIR}/../testrunner-images/${HARNESS_IMAGE}"

echo "**********************************"
echo "* Preparing test runner image"
echo "**********************************"

case $TARGET in

    "x86_64-unknown-linux-gnu")
        dd if=/dev/null of="${IMG_PATH}" bs=1M seek="${HARNESS_SIZE_MB}"
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
        dd if=/dev/null of="${IMG_PATH}" bs=1M seek="${HARNESS_SIZE_MB}"
        mformat -F -i "${IMG_PATH}" "::"
        mcopy \
            -i "${IMG_PATH}" \
            "${SCRIPT_DIR}/../target/x86_64-pc-windows-gnu/release/test-runner.exe" \
            "${SCRIPT_DIR}/../packages/"*.exe \
            "::"
        mdir -i "${IMG_PATH}"
        ;;

    *-darwin)
        rm -f "${IMG_PATH}"

        hdiutil create -size "${HARNESS_SIZE_MB}m" "${IMG_PATH}" \
            -volname testing \
            -fs HFS+J

        MOUNTPOINT=$(mktemp -d)
        hdiutil attach -mountpoint "${MOUNTPOINT}" "${IMG_PATH}"

        trap "hdiutil detach "${MOUNTPOINT}"" EXIT

        cp "${SCRIPT_DIR}/../target/$TARGET/release/test-runner" \
            "${SCRIPT_DIR}/../packages/"*.pkg \
            "${MOUNTPOINT}/"

        ;;

esac

echo "**********************************"
echo "* Success!"
echo "**********************************"
