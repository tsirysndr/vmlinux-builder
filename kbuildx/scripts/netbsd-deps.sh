#!/bin/sh
set -eu

export PATH=/usr/pkg/bin:/usr/pkg/sbin:/sbin:/usr/sbin:/bin:/usr/bin

# The microVM has no RTC the kernel trusts, so NetBSD comes up with a stale
# clock (the image's build date) and every TLS certificate is "not yet
# valid" — git's clone of the source tree died on exactly that. NTP first;
# failing that, the Date header of a plain-HTTP response from the CDN that
# pkgin fetches from anyway.
sync_clock() {
    ntpdate -b -t 5 pool.ntp.org >/dev/null 2>&1 && return 0
    hdr=$(curl -sI --max-time 10 http://cdn.NetBSD.org/ 2>/dev/null | tr -d '\r' | sed -n 's/^[Dd]ate: //p' | head -n 1)
    [ -n "$hdr" ] || return 1
    stamp=$(printf '%s\n' "$hdr" | awk '{
        split("Jan Feb Mar Apr May Jun Jul Aug Sep Oct Nov Dec", m, " ");
        for (i = 1; i <= 12; i++) if (m[i] == $3) mon = i;
        split($5, t, ":");
        printf "%04d%02d%02d%02d%02d.%02d", $4, mon, $2, t[1], t[2], t[3] }')
    date -u "$stamp" >/dev/null 2>&1
}
# Base NetBSD has ntpdate but no curl, so this first pass can only try NTP;
# the HTTP fallback gets its second chance below, once pkgin has fetched curl.
sync_clock || true
printf 'clock: %s\n' "$(date -u)"

data_disk=/dev/ld1a
data_raw=/dev/rld1a
if ! mount | grep -q ' /root/kbuildx '; then
    mkdir -p /root/kbuildx
    if fstyp "$data_raw" 2>/dev/null | grep -q ffs; then
        printf '%s\n' 'Mounting existing NetBSD build disk'
        mount "$data_disk" /root/kbuildx
    else
        printf '%s\n' 'Formatting NetBSD build disk'
        # Optimisé pour libkrun : UFS2, blocs standards, densité d'inodes équilibrée
        newfs -O 2 -b 16384 -f 2048 -i 32768 -m 1 "$data_disk"
        printf '%s\n' 'Mounting new NetBSD build disk'
        mount "$data_disk" /root/kbuildx
    fi
fi

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
    df -i /
    resize_ffs -y /dev/rld0a
    df -h /
    df -i /
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

ls -ld /usr /usr/pkg /usr/pkg/lib
mkdir -p /usr/pkg/bin /usr/pkg/sbin /usr/pkg/etc

# The stock image (base + comp sets) is self-hosting for build.sh; pkgsrc is
# only needed for git (source fetch) and the TLS roots it pulls in.
export ASSUME_ALWAYS_YES=yes
pkgin update
pkgin -y install git mozilla-rootcerts-openssl \
    || cat /var/db/pkgin/pkg_install-err.log
if command -v mozilla-rootcerts >/dev/null 2>&1; then
    mozilla-rootcerts install
elif [ -x /usr/pkg/sbin/mozilla-rootcerts ]; then
    /usr/pkg/sbin/mozilla-rootcerts install
fi

git --version

# Second pass with curl available (see sync_clock above): the clone over HTTPS
# that follows is the first thing here that checks a certificate's dates.
sync_clock || printf '%s\n' 'warning: could not sync the clock; HTTPS fetches may fail' >&2
printf 'clock: %s\n' "$(date -u)"
