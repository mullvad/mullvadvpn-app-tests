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

* Get the latest stable Rust from https://rustup.rs/.

* For running tests on Linux and Windows guests, you will need these tools and libraries:

    ```
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

Here is an example of how to create a new OS configuration and then run all tests:

```bash
# Create or edit configuration
test-manager set debian11 qemu ./os-images/debian11.qcow2 linux \
    --package-type deb --architecture x64 \
    --artifacts-dir /opt/testing \
    --disks ./testrunner-images/linux-test-runner.img

# Try it out to see if it works
#test-manager run debian11

# Run all tests
test-manager run-tests debian11 \
    --display \
    --account 0123456789 \
    --current-app abc123 \
    --previous-app 2023.2
```

## Note on `ci-runtests.sh`

Account tokens are read (newline-delimited) from the path specified by the environment variable
`ACCOUNT_TOKENS`. Round robin is used to select an account for each VM.
