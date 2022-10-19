# Running tests
## Dependencies
You will need `glibc-static` and `e2tools` on fedora to launch the guest VM.

## Running a test
Start the test VM by running `./launch-guest.sh` and inputting your password.
In the test window output you will find the serial bus path which looks something like `/dev/pts/1`, copy this path.
In a new terminal run `cargo build --bin test-manager` and then `sudo ./target/debug/test-manager /dev/pts/7 clean-app-install` to run the `clean-app-install` test.

## Seeing the output
In the guest you can see the output by running `sudo journalctl -f -u testrunner`
