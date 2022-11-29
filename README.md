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

* To run tests on Linux guests, you will need `glibc-static` and `e2tools`. On Fedora, install them
  using

    ```
    dnf install glibc-static e2tools
    ```

* To run tests on Windows guests, you'll need some toolchains and libraries, and `mtools`:

    ```
    rustup target add x86_64-pc-windows-gnu
    dnf install mingw64-gcc mingw64-winpthreads-static mtools
    ```

* `rootlesskit` is used to set up an isolated network namespace with `slirp`,
  and `dnsmasq` to assign IPs to VMs/containers.

    ```
    dnf install golang-github-rootless-containers-rootlesskit dnsmasq
    ```

## Building test-runner image

You must get a `.deb` or `.exe` of the Mullvad App from https://releases.mullvad.net/releases/ in
order to load into the testing environment.
Put the `.deb` or `.exe` in the `packages/` directory then create two symbolic links called
`current-app.deb/exe` and `previous-app.deb/exe` in the same directory pointing to the downloaded
Mullvad App `.deb` or `.exe` file.

Then build with:
```
./build.sh
```

# Building base images

See [`BUILD_BASE_IMAGE.md`](./BUILD_BASE_IMAGE.md) for how to build images for running tests on.

# Running tests
Run all tests on Debian using `./runtests.sh`. To run the tests on Windows (on a Linux host), use
`TARGET=x86_64-pc-windows-gnu ./runtests.sh`.

To run the tests on ARM64 macOS (on a *macOS* host), use
`TARGET=aarch64-apple-darwin ./runtests.sh`.

# Seeing the output
In the guest you can see the output by running `sudo journalctl -f -u testrunner`
