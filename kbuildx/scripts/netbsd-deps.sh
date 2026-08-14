#!/bin/sh
set -eu

export PATH=/usr/pkg/bin:/usr/pkg/sbin:/sbin:/usr/sbin:/bin:/usr/bin

package_release=${1:-}

case "$(uname -m)" in
    amd64|x86_64) pkg_arch=x86_64 ;;
    aarch64|arm64) pkg_arch=aarch64 ;;
    *)
        printf '%s\n' "Unsupported NetBSD architecture: $(uname -m)" >&2
        exit 1
        ;;
esac

host_release=$(uname -r)

case "$package_release" in
    current|trunk|"")
        case "$host_release" in
            11.99.*)
                # There are no official NetBSD-current binary packages.
                # Use the latest NetBSD 11 binary package set as a best-effort
                # compatibility repository.
                package_release=11.0_2026Q2
                ;;
            *)
                package_release=${host_release%%.*}.0
                ;;
        esac
        ;;
esac

package_path="http://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$package_release/All/"

printf '%s\n' "NetBSD host: $(uname -r)"
printf '%s\n' "pkgsrc repository: $package_path"

if command -v resize_ffs >/dev/null 2>&1; then
    printf '%s\n' 'Growing NetBSD FFS root filesystem to fill the virtual disk'
    df -h /
    resize_ffs -y /dev/rld0a
    df -h /
else
    printf '%s\n' 'NetBSD resize_ffs is unavailable; cannot use expanded build disk' >&2
    exit 1
fi

export PKG_PATH="$package_path"

if ! command -v pkgin >/dev/null 2>&1; then
    printf '%s\n' 'Bootstrapping pkgin with pkg_add'
    pkg_add pkgin
fi

mkdir -p /usr/pkg/etc/pkgin
printf '%s\n' "$package_path" > /usr/pkg/etc/pkgin/repositories.conf

pkgin -y update
pkgin -y install git
git --version
