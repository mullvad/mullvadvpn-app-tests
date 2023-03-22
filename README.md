# Project structure

## test-manager

The client part of the testing environment. This program runs on the host and connects over a
virtual serial port to the `test-runner`.

The tests themselves are defined in this package, using the interface provided by `test-runner`.

## test-runner

The server part of the testing environment. This program runs in guest VMs and provides the
`test-manager` with the building blocks (RPCs) needed to create tests.

## test-rpc

A support library for the other two packages. Defines an RPC interface, transports, shared types,
etc.

# Prerequisities

For macOS, the host machine must be macOS. All other platforms assume that the host is Linux.

## All platforms

* Get the latest stable Rust from https://rustup.rs/.

## macOS

Normally, you would use Tart here. It can be installed with Homebrew:

    ```bash
    brew install cirruslabs/cli/tart
    ```

## Linux

* For running tests on Linux and Windows guests, you will need these tools and libraries:

    ```bash
    dnf install git gcc protobuf-devel libpcap-devel qemu \
        podman e2tools mingw64-gcc mingw64-winpthreads-static mtools \
        golang-github-rootless-containers-rootlesskit slirp4netns dnsmasq \
        dbus-devel pkgconf-pkg-config swtpm edk2-ovmf \
        wireguard-tools

    rustup target add x86_64-pc-windows-gnu
    ```

# Building base images

See [`BUILD_OS_IMAGE.md`](./BUILD_OS_IMAGE.md) for how to build images for running tests on.

# Running tests

See `cargo run --bin test-manager` for details.

## Linux

Here is an example of how to create a new OS configuration and then run all tests:

```bash
# Create or edit configuration
# The image is assumed to contain a test runner service set up as described in ./BUILD_OS_IMAGE.md
test-manager set debian11 qemu ./os-images/debian11.qcow2 linux \
    --package-type deb --architecture x64 \
    --artifacts-dir /opt/testing \
    --disks ./testrunner-images/linux-test-runner.img

# Try it out to see if it works
#test-manager run-vm debian11

# Run all tests
test-manager run-tests debian11 \
    --display \
    --account 0123456789 \
    --current-app <git hash or tag> \
    --previous-app 2023.2
```

## macOS

Here is an example of how to create a new OS configuration (on Apple Silicon) and then run all
tests:

```bash
# Download some VM image
tart clone ghcr.io/cirruslabs/macos-ventura-base:latest ventura-base

# Create or edit configuration
# Use SSH to deploy the test runner since the image doesn't contain a runner
test-manager set macos-ventura tart ventura-base macos \
    --architecture aarch64 \
    --provisioner ssh --ssh-user admin --ssh-password admin

# Try it out to see if it works
#test-manager run-vm macos-ventura

# Run all tests
test-manager run-tests macos-ventura \
    --display \
    --account 0123456789 \
    --current-app <git hash or tag> \
    --previous-app 2023.2
```

## Note on `ci-runtests.sh`

Account tokens are read (newline-delimited) from the path specified by the environment variable
`ACCOUNT_TOKENS`. Round robin is used to select an account for each VM.
