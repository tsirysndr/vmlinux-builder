#!/bin/sh
set -eu
data_disk=/dev/vtbd1
if ! mount | grep -q ' /root/kbuildx '; then
    if ! fsck -n "$data_disk" >/dev/null 2>&1; then
        newfs "$data_disk"
    fi
    mkdir -p /root/kbuildx
    mount "$data_disk" /root/kbuildx
fi

marker=/var/db/kbuildx-root-grown
if [ ! -e "$marker" ]; then
    printf '%s\n' 'Growing FreeBSD root filesystem to fill the virtual disk'
    growfs -y /
    touch "$marker"
fi
df -h /
env ASSUME_ALWAYS_YES=yes pkg install -y git ca_root_nss
