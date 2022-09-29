#!/usr/bin/env bash

VIRTUAL_NET=172.29.1.1/24

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

# In VM (example):
#ip a add 172.29.1.2/24
#ip r add default via 172.29.1.1
