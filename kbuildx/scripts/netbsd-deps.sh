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
bootstrap_path="http://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$pkg_release/All/"
secure_path="https://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$pkg_release/All/"
if ! command -v pkgin >/dev/null 2>&1; then
    printf '%s\n' 'Bootstrapping pkgin with pkg_add'
    export PKG_PATH="$bootstrap_path"
    pkg_add pkgin
fi
if [ ! -e /etc/openssl/certs/ca-certificates.crt ] && [ ! -e /etc/ssl/cert.pem ]; then
    printf '%s\n' 'Bootstrapping CA certificates with pkg_add'
    mkdir -p /etc/openssl/certs
    export PKG_PATH="$bootstrap_path"
    pkg_add mozilla-rootcerts-openssl
fi
if command -v mozilla-rootcerts >/dev/null 2>&1; then
    mozilla-rootcerts install
fi
mkdir -p /usr/pkg/etc/pkgin
export PKG_PATH="$secure_path"
printf '%s\n' "$secure_path" > /usr/pkg/etc/pkgin/repositories.conf
pkgin -y update
pkgin -y install git-base mozilla-rootcerts-openssl || pkgin -y install git
