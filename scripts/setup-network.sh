#!/usr/bin/env bash

set -eu

ip link add br-mullvadtest type bridge
ip tuntap add tap-mullvadtest mode tap

ip link set tap-mullvadtest master br-mullvadtest

ip addr add dev br-mullvadtest 172.29.1.1/24

ip link set br-mullvadtest up
ip link set tap-mullvadtest up

# In VM:
#ip a add 172.29.1.2/24
#ip r add default via 172.29.1.1
