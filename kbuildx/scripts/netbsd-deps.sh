#!/bin/sh
set -eu

version=$1
export PATH=/usr/pkg/bin:/usr/pkg/sbin:/sbin:/usr/sbin:/bin:/usr/bin
if command -v resize_ffs >/dev/null 2>&1; then
    printf '%s\n' 'Growing NetBSD FFS root filesystem to fill the virtual disk'
    df -h /
    resize_ffs -y /dev/rld0a
    df -h /
else
    printf '%s\n' 'NetBSD resize_ffs is unavailable; cannot use the expanded build disk' >&2
    exit 1
fi
case "$(uname -m)" in
    amd64|x86_64) pkg_arch=x86_64 ;;
    *) pkg_arch=aarch64 ;;
esac
host_release=$(uname -r)
case "$host_release" in
    11.99.*) pkg_release=11.0 ;;
    10.*) pkg_release=${host_release%%-*} ;;
    *) pkg_release=${host_release%%-*} ;;
esac
package_path="http://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$pkg_release/All/"
# The prepared bsdkrun NetBSD image has no CA bundle and cannot install one
# into its read-only certificate layout; use the official CDN over HTTP for
# this disposable CI build guest so pkg_add/pkgin can bootstrap successfully.
if ! command -v pkgin >/dev/null 2>&1; then
    printf '%s\n' 'Bootstrapping pkgin with pkg_add'
    export PKG_PATH="$package_path"
    pkg_add pkgin
fi
mkdir -p /usr/pkg/etc/pkgin
export PKG_PATH="$package_path"
printf '%s\n' "$package_path" > /usr/pkg/etc/pkgin/repositories.conf
pkgin -y update
secure_path="https://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$pkg_release/All/"
if [ -e /etc/ssl/cert.pem ] || [ -e /etc/openssl/certs/ca-certificates.crt ]; then
    package_path="$secure_path"
    export PKG_PATH="$package_path"
    printf '%s\n' "$package_path" > /usr/pkg/etc/pkgin/repositories.conf
    pkgin -y update
fi
if ! pkgin -y install git-base; then
    # NetBSD pkg_install can report post-install warnings for base files while
    # still completing the requested package. Continue only when Git is usable.
    if ! command -v git >/dev/null 2>&1; then
        printf '%s\n' 'pkgin failed and git is unavailable' >&2
        exit 1
    fi
    printf '%s\n' 'pkgin reported warnings; git-base is available, continuing'
fi
