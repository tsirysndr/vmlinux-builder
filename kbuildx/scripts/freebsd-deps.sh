#!/bin/sh
set -eu

marker=/var/db/kbuildx-root-grown
if [ ! -e "$marker" ]; then
    printf '%s\n' 'Growing FreeBSD root filesystem to fill the virtual disk'
    growfs -y /
    touch "$marker"
fi
df -h /
env ASSUME_ALWAYS_YES=yes pkg install -y git ca_root_nss
