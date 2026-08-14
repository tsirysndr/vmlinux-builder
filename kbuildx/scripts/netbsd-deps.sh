#!/bin/sh
set -eu

export PATH=/usr/pkg/bin:/usr/pkg/sbin:/sbin:/usr/sbin:/bin:/usr/bin
package_release=${1:-}
case "$package_release" in current|trunk) package_release=;; esac
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
    aarch64|arm64) pkg_arch=aarch64 ;;
    *) printf '%s\n' "Unsupported NetBSD architecture: $(uname -m)" >&2; exit 1 ;;
esac
if [ -z "$package_release" ]; then
    host_release=$(uname -r)
    package_release=${host_release%%.*}.0
fi
package_path="http://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/$pkg_arch/$package_release/All/"
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
if ! pkgin -y install git-base; then
    # pkgin may roll back the transaction after a non-essential package
    # post-install warning. Retry the requested package directly, forcing the
    # install from the already configured binary repository.
    printf '%s\n' 'pkgin failed; retrying git-base with pkg_add -f'
    pkg_add -f git-base || true
    if ! command -v git >/dev/null 2>&1; then
        printf '%s\n' 'pkgin failed and git is unavailable' >&2
        exit 1
    fi
    printf '%s\n' 'pkgin reported warnings; git-base is available, continuing'
fi
