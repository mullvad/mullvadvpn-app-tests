TODO: Automate the creation of the base image

This document explains how to create base OS images and run test runners on them.

For macOS, the host machine must be macOS. All other platforms assume that the host is Linux.

# Creating a base Debian image

On the host, start by creating a disk image and installing Debian on it:

```
wget https://cdimage.debian.org/debian-cd/current/amd64/iso-cd/debian-11.5.0-amd64-netinst.iso
mkdir -p os-images
qemu-img create -f qcow2 ./os-images/debian.qcow2 5G
qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 -cdrom debian-11.5.0-amd64-netinst.iso -drive file=./os-images/debian.qcow2
```

## Bootstrapping RPC server

The testing image needs to be mounted to `/opt/testing`, and the RPC server needs to be started on boot.
This can be achieved as follows:

* (If needed) start the VM:

    ```
    qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 -drive file=./os-images/debian.qcow2
    ```

* In the guest, create a mount point for the runner: `mkdir -p /opt/testing`.

* Add an entry to `/etc/fstab`:

    ```
    # Mount testing image
    /dev/sdb /opt/testing ext4 defaults 0 1
    ```

* Create a systemd service that starts the RPC server, `/etc/systemd/system/testrunner.service`:

    ```
    [Unit]
    Description=Mullvad Test Runner

    [Service]
    ExecStart=/opt/testing/test-runner /dev/ttyS0 serve

    [Install]
    WantedBy=multi-user.target
    ```

* Enable the service: `systemctl enable testrunner.service`.

## Running tests
### Dependencies
You will need `glibc-static` and `e2tools` on fedora to launch the guest VM.

### Running a test
Start the test VM by running `./launch-guest.sh` and inputting your password.
In the test window output you will find the serial bus path which looks something like `/dev/pts/1`, copy this path.
In a new terminal run `cargo build --bin test-manager` and then `sudo ./target/debug/test-manager /dev/pts/7 clean-app-install` to run the `clean-app-install` test.

### Seeing the output
In the guest you can see the output by running `sudo journalctl -f -u testrunner`

# Creating a base Windows 10 image

* Download a Windows ISO: https://www.microsoft.com/software-download/windows10

* On the host, create a new disk image and install Windows on it:

    ```
    mkdir -p os-images
    qemu-img create -f qcow2 ./os-images/windows10.qcow2 32G
    qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 -cdrom <YOUR ISO HERE> -drive file=./os-images/windows10.qcow2
    ```

## Bootstrapping RPC server

The testing image needs to be mounted to `E:`, and the RPC server needs to be started on boot.
This can be achieved as follows:

* (If needed) start the VM:

    ```
    qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
        -drive file=./os-images/windows10.qcow2 \
        -drive file=./testrunner-images/windows-test-runner.img
    ```

* In the guest, add the test runner as a scheduled task:

    ```
    schtasks /create /tn "Mullvad Test Runner" /sc onlogon /tr "\"E:\test-runner.exe\" \\.\COM1 serve" /rl highest
    ```

* Shut down without logging out.

TODO: Replace with a service? Might want user session, though.

# Creating a base macOS image (macOS only)

[UTM](https://mac.getutm.app/) is currently required. It is very limited due to the lack of a CLI
interface.

* Create a macOS VM in UTM. Rename it to `mullvad-macOS`.

* Edit the VM:

  * Go to System > Advanced and check "Enable Serial".

  * Import the `macos-test-runner.dmg` drive. This must be done after running `build.sh`.

* Launch the VM and complete the installation of macOS.

## Bootstrapping RPC server

* In the guest, create a service that starts the RPC server, `/Library/LaunchDaemons/net.mullvad.testunner.plist`:

    ```
    <?xml version="1.0" encoding="UTF-8"?>
    <!DOCTYPE plist PUBLIC "-//Apple Computer//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
    <plist version="1.0">
    <dict>
        <key>Label</key>
        <string>net.mullvad.testrunner</string>

        <key>ProgramArguments</key>
        <array>
            <string>/Volumes/testing/test-runner</string>
            <string>/dev/tty.virtio</string>
            <string>serve</string>
        </array>

        <key>UserName</key>
        <string>root</string>

        <key>RunAtLoad</key>
        <true/>

        <key>KeepAlive</key>
        <true/>

        <key>StandardOutPath</key>
        <string>/tmp/runner.out</string>

        <key>StandardErrorPath</key>
        <string>/tmp/runner.err</string>
    </dict>
    </plist>
    ```

* Enable the service: `sudo launchctl load -w /Library/LaunchDaemons/net.mullvad.testunner.plist`

* Shut down the guest.

FIXME: Patch tokio-serial due to baud rate error
