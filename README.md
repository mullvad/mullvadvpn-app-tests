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
        dbus-devel pkgconf-pkg-config swtpm edk2-ovmf

    rustup target add x86_64-pc-windows-gnu
    ```

# Building base images

See [`BUILD_OS_IMAGE.md`](./BUILD_OS_IMAGE.md) for how to build images for running tests on.

# Running tests
Run all tests on Debian 11 using `OS=debian11 ./runtests.sh`. To run the tests on Windows 10 (on
a Linux host), use `OS=windows10 ./runtests.sh`.

## Environment variables

* `ACCOUNT_TOKENS` - Comma-separated list of account numbers. Use instead of `ACCOUNT_TOKEN` for
  `./ci-runtests.sh`. Uses round robin to select an account for each VM.

* `ACCOUNT_TOKEN` - Must be set to a valid Mullvad account number since a lot of tests depend on
  the app being logged in.

* `SHOW_DISPLAY` - Setting this prevents the tests from running "headless". It also prevents the
  guest VM from being killed once the tests have finished running.

* `PREVIOUS_APP_FILENAME` - This should be set to the filename of a package in `./packages/`. It
  will be used to install the previous app version and is used for testing upgrades to the version
  under test.

* `CURRENT_APP_FILENAME` - This should be set to the filename of a package in `./packages/`. It
  should contain the app version under test.

* `UI_E2E_TESTS_FILENAME` - This should be set to the filename of the E2E test executable in
  `./packages/`. See [here](https://github.com/mullvad/mullvadvpn-app/blob/main/gui/README.md#standalone-test-executable)
  for details.
