#!/usr/bin/env bash

set -eu

VIRTUAL_NET=172.29.1.1/24
VIRTUAL_NET_IP_FIRST=172.29.1.2
VIRTUAL_NET_IP_LAST=172.29.1.128

ip link show br-mullvadtest >&/dev/null && exit 0

sysctl net.ipv4.ip_forward=1

ip link add br-mullvadtest type bridge
ip addr add dev br-mullvadtest $VIRTUAL_NET
ip link set br-mullvadtest up

# add NAT rule
nft -f - <<EOF
table ip mullvad_test_nat {
    chain POSTROUTING {
        type nat hook postrouting priority srcnat; policy accept;
        ip saddr $VIRTUAL_NET ip daddr != $VIRTUAL_NET counter masquerade
    }
}
EOF

# set up pingable hosts
ip link add lan-mullvadtest type dummy
ip addr add dev lan-mullvadtest 172.29.1.200
ip link add net-mullvadtest type dummy
ip addr add dev net-mullvadtest 1.3.3.7

# start DHCP server
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
dnsmasq -i br-mullvadtest -F "${VIRTUAL_NET_IP_FIRST},${VIRTUAL_NET_IP_LAST}" -x "${SCRIPT_DIR}/.dnsmasq.pid" -l "${SCRIPT_DIR}/.dnsmasq.leases"
