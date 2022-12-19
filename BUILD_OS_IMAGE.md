This document explains how to create base OS images and run test runners on them.

For macOS, the host machine must be macOS. All other platforms assume that the host is Linux.

# Creating a base Linux image

These instructions use Debian, but the process is pretty much the same for any other distribution.

On the host, start by creating a disk image and installing Debian on it:

```
wget https://cdimage.debian.org/debian-cd/current/amd64/iso-cd/debian-11.5.0-amd64-netinst.iso
mkdir -p os-images
qemu-img create -f qcow2 ./os-images/debian.qcow2 5G
qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 -cdrom debian-11.5.0-amd64-netinst.iso -drive file=./os-images/debian.qcow2
```

## Bootstrapping test runner

The testing image needs to be mounted to `/opt/testing`, and the test runner needs to be started on
boot.

* In the guest, create a mount point for the runner: `mkdir -p /opt/testing`.

* Add an entry to `/etc/fstab`:

    ```
    # Mount testing image
    /dev/sdb /opt/testing ext4 defaults 0 1
    ```

* Create a systemd service that starts the test runner, `/etc/systemd/system/testrunner.service`:

    ```
    [Unit]
    Description=Mullvad Test Runner

    [Service]
    ExecStart=/opt/testing/test-runner /dev/ttyS0 serve

    [Install]
    WantedBy=multi-user.target
    ```

* Enable the service: `systemctl enable testrunner.service`.

# Creating a base Windows 10 image

* Download a Windows ISO: https://www.microsoft.com/software-download/windows10

* On the host, create a new disk image and install Windows on it:

    ```
    mkdir -p os-images
    qemu-img create -f qcow2 ./os-images/windows10.qcow2 32G
    qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 -cdrom <YOUR ISO HERE> -drive file=./os-images/windows10.qcow2
    ```

## Bootstrapping test runner

The test runner needs to be started on boot, with the test runner image mounted at `E:`.
This can be achieved as follows:

* Restart the VM:

    ```
    qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 \
        -drive file="./os-images/windows10.qcow2" \
        -device nec-usb-xhci,id=xhci \
        -device usb-storage,drive=runner,bus=xhci.0
    ```

* In the guest admin `cmd`, add the test runner as a scheduled task:

    ```
    schtasks /create /tn "Mullvad Test Runner" /sc onlogon /tr "\"E:\test-runner.exe\" \\.\COM1 serve" /rl highest
    ```

* In the guest, disable Windows Update.

    * Open `services.msc`.

    * Open the properties for `Windows Update`.

    * Set "Startup type" to "Disabled". Also, click "stop".

* In the guest, disable SmartScreen.

    * Go to "Reputation-based protection settings" under
      Start > Settings > Update & Security > Windows Security > App & browser control.

    * Set "Check apps and files" to off.

* Shut down without logging out.

# Creating a base macOS image (macOS only)

[UTM](https://mac.getutm.app/) is currently required. It is very limited due to the lack of a CLI
interface.

* Create a macOS VM in UTM. Rename it to `mullvad-macOS`.

* Edit the VM:

  * Go to System > Advanced and check "Enable Serial".

  * Import the `macos-test-runner.dmg` drive. This must be done after running `build.sh`.

* Launch the VM and complete the installation of macOS.

## Bootstrapping test runner

* In the guest, create a service that starts the test runner,
  `/Library/LaunchDaemons/net.mullvad.testunner.plist`:

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
