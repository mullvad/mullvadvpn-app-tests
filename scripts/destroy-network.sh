#!/usr/bin/env bash

set -u

exec 2> /dev/null

ip link del lan-mullvadtest
ip link del net-mullvadtest
ip link del br-mullvadtest
ip link del tap-mullvadtest
nft delete table ip mullvad_test_nat

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"

dnsmasq_pid=$(cat "${SCRIPT_DIR}/.dnsmasq.pid")
if [[ $? -eq 0 ]]; then
    kill -- ${dnsmasq_pid}
    rm -f "${SCRIPT_DIR}/.dnsmasq.pid"
fi
