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

* To run tests on Linux guests, you will need `glibc-static` and `e2tools`. On Fedora, install them using

    ```
    dnf install glibc-static e2tools
    ```

* To run tests on Windows guests, you'll need some toolchains and libraries, and `mtools`:

    ```
    rustup target add x86_64-pc-windows-gnu
    dnf install mingw64-gcc mingw64-winpthreads-static mtools
    ```

# Building base images

See [`BUILD_BASE_IMAGE.md`](./BUILD_BASE_IMAGE.md) for how to build images for running tests on.

# Running a test
Start the test VM by running `./launch-guest.sh` and inputting your password.
In the test window output you will find the serial bus path which looks something like `/dev/pts/1`, copy this path.
In a new terminal run `cargo build --bin test-manager` and then `sudo ./target/debug/test-manager /dev/pts/7 clean-app-install` to run the `clean-app-install` test.

# Seeing the output
In the guest you can see the output by running `sudo journalctl -f -u testrunner`
