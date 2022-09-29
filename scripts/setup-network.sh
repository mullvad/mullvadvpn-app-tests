#!/usr/bin/env bash

VIRTUAL_NET=172.29.1.1/24
VIRTUAL_NET_IP_FIRST=172.29.1.2
VIRTUAL_NET_IP_LAST=172.29.1.254

set -eu

ip link add br-mullvadtest type bridge
ip tuntap add tap-mullvadtest mode tap

ip link set tap-mullvadtest master br-mullvadtest

ip addr add dev br-mullvadtest $VIRTUAL_NET

ip link set br-mullvadtest up
ip link set tap-mullvadtest up

# add NAT rule
nft -f - <<EOF
table ip mullvad_test_nat {
    chain POSTROUTING {
        type nat hook postrouting priority srcnat; policy accept;
        ip saddr $VIRTUAL_NET ip daddr != $VIRTUAL_NET counter masquerade
    }
}
EOF

if systemctl status firewalld >&/dev/null; then
    firewall-cmd --zone=trusted --change-interface=br-mullvadtest
fi

# start DHCP server
dnsmasq -i br-mullvadtest -F "${VIRTUAL_NET_IP_FIRST},${VIRTUAL_NET_IP_LAST}"
