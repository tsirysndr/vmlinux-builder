#!/bin/sh
set -eu

version=$1
export PATH=/usr/pkg/bin:/usr/pkg/sbin:/sbin:/usr/sbin:/bin:/usr/bin
case "$(uname -m)" in
    amd64|x86_64) pkg_arch=x86_64 ;;
    *) pkg_arch=aarch64 ;;
esac
case "$version" in
    current|trunk) pkg_release=10.1 ;;
    *) pkg_release=${version%%-*} ;;
esac
export PKG_PATH="https://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$pkg_release/All/"
if ! command -v pkgin >/dev/null 2>&1; then
    printf '%s\n' 'Bootstrapping pkgin with pkg_add'
    pkg_add pkgin
fi
mkdir -p /usr/pkg/etc/pkgin
printf '%s\n' "$PKG_PATH" > /usr/pkg/etc/pkgin/repositories.conf
pkgin -y update
pkgin -y install git-base mozilla-rootcerts-openssl || pkgin -y install git
