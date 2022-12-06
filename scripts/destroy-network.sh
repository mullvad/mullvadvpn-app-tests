#!/usr/bin/env bash

set -u

exec 2> /dev/null

ip link del lan-mullvadtest
ip link del net-mullvadtest
ip link del br-mullvadtest

for iface in $( ip -o -br link | grep tap-mullvad | cut -d' ' -f1 ); do
    echo "removing $iface"
    ip link del dev $iface
done

nft delete table ip mullvad_test_nat

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

dnsmasq_pid=$(cat "${SCRIPT_DIR}/.dnsmasq.pid")
if [[ $? -eq 0 ]]; then
    env kill -- ${dnsmasq_pid}
    rm -f "${SCRIPT_DIR}/.dnsmasq.pid"
fi
