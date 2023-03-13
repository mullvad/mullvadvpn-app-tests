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

# create wireguard relay

# NOTE: This relay does not support PQ handshakes, etc.
#
# The client should connect to 192.168.15.1 using this private key:
# mPue6Xt0pdz4NRAhfQSp/SLKo7kV7DW+2zvBq0N9iUI=
#
# The public key of the peer is 7svBwGBefP7KVmH/yes+pZCfO6uSOYeGieYYa1+kZ0E=.
#
# The endpoint is 172.29.1.200:51820

temp_wg_conf=$(mktemp)

cat <<CONF > $temp_wg_conf

[Interface]
PrivateKey = gLvQuyqazziyf+pUCAFUgTnWIwn6fPE5MOReOqPEGHU=
ListenPort = 51820

[Peer]
PublicKey = h6elqt3dfamtS/p9jxJ8bIYs8UW9YHfTFhvx0fabTFo=
AllowedIPs = 192.168.15.2

CONF

trap "rm $temp_wg_conf" EXIT TERM

ip link add dev wg-relay0 type wireguard
ip addr add dev wg-relay0 192.168.15.1 peer 192.168.15.2
wg setconf wg-relay0 ${temp_wg_conf}
ip link set up dev wg-relay0

# start DHCP server
SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
dnsmasq -i br-mullvadtest -F "${VIRTUAL_NET_IP_FIRST},${VIRTUAL_NET_IP_LAST}" -x "${SCRIPT_DIR}/.dnsmasq.pid" -l "${SCRIPT_DIR}/.dnsmasq.leases"
