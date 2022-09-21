TODO: Automate the creation of the base image

This document explains how to create a base image for Debian and bootstrap the test runner on it.

# Create a base Debian image

Start by creating a disk image and installing Debian on it:

```
wget https://cdimage.debian.org/debian-cd/current/amd64/iso-cd/debian-11.5.0-amd64-netinst.iso
qemu-img create ./qemu-images/debian.img 5G
qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 -cdrom debian-11.5.0-amd64-netinst.iso -drive file=./qemu-images/debian.img
```

# Bootstrap RPC server

The testing image needs to be mounted to `/opt/testing`, and the RPC server needs to be started on boot.
This can be achieved as follows:

* (If needed) start the VM:

    ```
    qemu-system-x86_64 -cpu host -accel kvm -m 2048 -smp 2 -drive file=./qemu-images/debian.img
    ```

* Create a mount point for the runner: `mkdir -p /opt/testing`.

* Add an entry to `/etc/fstab`:

    ```
    # Mount testing image
    /dev/sdb /opt/testing ext4 defaults 0 1
    ```

* Create a systemd service that starts the RPC server, `/etc/systemd/system/testrunner.service`:

    ```
    [Unit]
    Description=Mullvad test runner

    [Service]
    ExecStart=/opt/testing/test-tarpc /dev/ttyS0 serve

    [Install]
    WantedBy=multi-user.target
    ```

* Enable the service: `systemctl enable testrunner.service`.
