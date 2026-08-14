#!/bin/sh
set -eu
data_disk=/dev/vtbd1
if ! mount | grep -q ' /root/kbuildx '; then
    mkdir -p /root/kbuildx
    if fstyp "$data_disk" 2>/dev/null | grep -q ufs; then
        printf '%s\n' 'Mounting existing FreeBSD build disk'
        mount "$data_disk" /root/kbuildx
    else
        printf '%s\n' 'Formatting FreeBSD build disk'
        newfs -b 32768 -f 4096 -i 16384 "$data_disk"
        printf '%s\n' 'Mounting new FreeBSD build disk'
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
