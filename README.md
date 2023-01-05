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
        glibc-static e2tools \
        mingw64-gcc mingw64-winpthreads-static mtools \
        golang-github-rootless-containers-rootlesskit slirp4netns dnsmasq \
        dbus-devel pkgconf-pkg-config swtpm edk2-ovmf

    rustup target add x86_64-pc-windows-gnu
    ```

# Building base images

See [`BUILD_OS_IMAGE.md`](./BUILD_OS_IMAGE.md) for how to build images for running tests on.

# Running tests
Run all tests on Debian using `./runtests.sh`. To run the tests on Windows (on a Linux host), use
`TARGET=x86_64-pc-windows-gnu ./runtests.sh`.

To run the tests on ARM64 macOS (on a *macOS* host), use
`TARGET=aarch64-apple-darwin ./runtests.sh`.

## Environment variables

* `ACCOUNT_TOKEN` - Must be set to a valid Mullvad account number since a lot of tests depend on
  the app being logged in.

* `SHOW_DISPLAY` - Setting this causes prevents the tests from running "headless". It also prevents
  the guest VM from being killed once the tests have finished running.

* `PREVIOUS_APP_FILENAME` - This should be a set to the filename of a package in `./packages/`. It
  will be used to install the previous app version and is used for testing upgrades to the version
  under test.

* `CURRENT_APP_FILENAME` - This should be a set to the filename of a package  in `./packages/`. It
  should contain the app version under test.

## Seeing the output
In the guest you can see the output by running `sudo journalctl -f -u testrunner`
