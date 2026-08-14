#!/bin/sh
set -eu
data_disk=/dev/vtbd1
if ! mount | grep -q ' /root/kbuildx '; then
    mkdir -p /root/kbuildx
    if ! mount "$data_disk" /root/kbuildx 2>/dev/null; then
        newfs "$data_disk"
        mount "$data_disk" /root/kbuildx
    fi
fi

marker=/var/db/kbuildx-root-grown
if [ ! -e "$marker" ]; then
    printf '%s\n' 'Growing FreeBSD root filesystem to fill the virtual disk'
    growfs -y /
    touch "$marker"
fi
df -h /
env ASSUME_ALWAYS_YES=yes pkg install -y git ca_root_nss
