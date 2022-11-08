#!/usr/bin/env bash

# This script produces a virtual disk containing the test runner binaries.
# The resulting disk, ../testrunner-images/{OS}-test-runner.img, should be
# mounted to:
# * /opt/testing for Linux guests.
# * E: for Windows guests.
# * /Volumes/testing for macOS guests.

set -eu

TEST_RUNNER_IMAGE_SIZE_MB=500

case $TARGET in
    "x86_64-unknown-linux-gnu")
        TEST_RUNNER_IMAGE_FILENAME=linux-test-runner.img
        ;;
    "x86_64-pc-windows-gnu")
        TEST_RUNNER_IMAGE_FILENAME=windows-test-runner.img
        ;;
    *-darwin)
        TEST_RUNNER_IMAGE_FILENAME=macos-test-runner.dmg
        ;;
    *)
        echo "Unknown target: $TARGET"
        exit 1
        ;;
esac

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

mkdir -p "${SCRIPT_DIR}/../testrunner-images/"
TEST_RUNNER_IMAGE_PATH="${SCRIPT_DIR}/../testrunner-images/${TEST_RUNNER_IMAGE_FILENAME}"

echo "**********************************"
echo "* Preparing test runner image"
echo "**********************************"

case $TARGET in

    "x86_64-unknown-linux-gnu")
        truncate -s "${TEST_RUNNER_IMAGE_SIZE_MB}M" "${TEST_RUNNER_IMAGE_PATH}"
        mkfs.ext4 -F "${TEST_RUNNER_IMAGE_PATH}"
        e2cp \
            -P 500 \
            "${SCRIPT_DIR}/../target/$TARGET/release/test-runner" \
            "${TEST_RUNNER_IMAGE_PATH}:/"
        e2cp \
            "${SCRIPT_DIR}/../packages/"*.deb \
            "${TEST_RUNNER_IMAGE_PATH}:/"
        ;;

    "x86_64-pc-windows-gnu")
        truncate -s "${TEST_RUNNER_IMAGE_SIZE_MB}M" "${TEST_RUNNER_IMAGE_PATH}"
        mformat -F -i "${TEST_RUNNER_IMAGE_PATH}" "::"
        mcopy \
            -i "${TEST_RUNNER_IMAGE_PATH}" \
            "${SCRIPT_DIR}/../target/$TARGET/release/test-runner.exe" \
            "${SCRIPT_DIR}/../packages/"*.exe \
            "::"
        mdir -i "${TEST_RUNNER_IMAGE_PATH}"
        ;;

    *-darwin)
        rm -f "${TEST_RUNNER_IMAGE_PATH}"

        hdiutil create -size "${TEST_RUNNER_IMAGE_SIZE_MB}m" "${TEST_RUNNER_IMAGE_PATH}" \
            -volname testing \
            -fs HFS+J

        MOUNTPOINT=$(mktemp -d)
        hdiutil attach -mountpoint "${MOUNTPOINT}" "${TEST_RUNNER_IMAGE_PATH}"

        trap "hdiutil detach "${MOUNTPOINT}"" EXIT

        cp "${SCRIPT_DIR}/../target/$TARGET/release/test-runner" \
            "${SCRIPT_DIR}/../packages/"*.pkg \
            "${MOUNTPOINT}/"

        ;;

esac

echo "**********************************"
echo "* Success!"
echo "**********************************"
