#!/bin/sh
set -eu

version=$1
export PATH=/usr/pkg/bin:/usr/pkg/sbin:/sbin:/usr/sbin:/bin:/usr/bin
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
if pkgin -y install mozilla-rootcerts-openssl; then
    if command -v mozilla-rootcerts >/dev/null 2>&1; then
        mozilla-rootcerts install || true
    fi
fi
secure_path="https://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$pkg_release/All/"
if [ -e /etc/ssl/cert.pem ] || [ -e /etc/openssl/certs/ca-certificates.crt ]; then
    package_path="$secure_path"
    export PKG_PATH="$package_path"
    printf '%s\n' "$package_path" > /usr/pkg/etc/pkgin/repositories.conf
    pkgin -y update
fi
pkgin -y install git-base || pkgin -y install git
