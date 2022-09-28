TODO: Automate the creation of the base image

This document explains how to create base QEMU images and run test runners on them.

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
