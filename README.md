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

For running tests on Linux and Windows guests, the recommended way is to use the included container
image:

```bash
podman build -t mullvadvpn-app-tests .
```

# Building base images

See [`BUILD_OS_IMAGE.md`](./BUILD_OS_IMAGE.md) for how to build images for running tests on.

# Running tests

Run all tests by setting the necessary environment variables and running `runtests.sh`:

```bash
OS=debian11 ACCOUNT_TOKEN=1234 \
CURRENT_APP_FILENAME=MullvadVPN-2023.1-dev-123abc_amd64.deb \
PREVIOUS_APP_FILENAME=MullvadVPN-2023.1_amd64.deb \
./container-run.sh ./runtests.sh
```

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

* `CURRENT_APP_FILENAME` - This should be set to the filename of a package  in `./packages/`. It
  should contain the app version under test.
