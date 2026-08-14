#!/bin/sh
set -eu

version=$1; repo=$2; ref=$3; label=$4; requested_config=$5; bundle=$6
work=/root/kbuildx
src=$work/freebsd-src
obj=$work/freebsd-obj
artifacts=$work/artifacts
mkdir -p "$work" "$artifacts"
if git -C "$src" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$src" fetch --force --depth 1 origin "$ref"
    git -C "$src" checkout --force --detach FETCH_HEAD
else
    rm -rf "$src"
    git clone --depth 1 --branch "$ref" "$repo" "$src"
fi
arch=$(uname -p)
host_arch=$(uname -m)
case "$host_arch" in amd64|x86_64) artifact_arch=x86_64 ;; *) artifact_arch=aarch64 ;; esac
config=$requested_config
if [ -z "$config" ]; then
    if [ "$bundle" = 1 ] && [ -f "$src/sys/amd64/conf/FIRECRACKER" ]; then
        config=FIRECRACKER
    else
        config=GENERIC
    fi
fi
rm -rf "$obj"
mkdir -p "$obj"
jobs=$(sysctl -n hw.ncpu)
env MAKEOBJDIRPREFIX="$obj" make -C "$src" -DNO_MODULES -j"$jobs" buildkernel KERNCONF="$config"
kernel=$(find "$obj" -type f -path "*/sys/$config/kernel" | head -n 1)
test -n "$kernel" -a -f "$kernel"
base="freebsd-${label}.${artifact_arch}"
cp "$kernel" "$artifacts/$base.kernel"
if [ "$bundle" = 1 ]; then
    root=$work/freebsd-rootfs
    rm -rf "$root"; mkdir -p "$root"
    release=${version%-RELEASE}
    fetch -o "$work/base.txz" "https://download.freebsd.org/releases/${arch}/${release}-RELEASE/base.txz"
    tar -xpf "$work/base.txz" -C "$root"
    env MAKEOBJDIRPREFIX="$obj" make -C "$src" -DNO_MODULES installkernel KERNCONF="$config" DESTDIR="$root"
    mkdir -p "$root/usr/local/sbin" "$root/usr/local/etc/rc.d"
    cp /usr/local/sbin/bsdkrun-agent "$root/usr/local/sbin/bsdkrun-agent"
    chmod 755 "$root/usr/local/sbin/bsdkrun-agent"
    cat > "$root/usr/local/etc/rc.d/bsdkrun_agent" <<'RC'
#!/bin/sh
# PROVIDE: bsdkrun_agent
# REQUIRE: NETWORKING
. /etc/rc.subr
name=bsdkrun_agent
rcvar=bsdkrun_agent_enable
command=/usr/sbin/daemon
pidfile=/var/run/bsdkrun_agent.pid
command_args="-f -P ${pidfile} -r /usr/local/sbin/bsdkrun-agent"
load_rc_config $name
run_rc_command "$1"
RC
    chmod 555 "$root/usr/local/etc/rc.d/bsdkrun_agent"
    sysrc -f "$root/etc/rc.conf" bsdkrun_agent_enable=YES
    sysrc -f "$root/etc/rc.conf" ifconfig_vtnet0=DHCP
    printf '/dev/vtbd0 / ufs rw 1 1\n' > "$root/etc/fstab"
    makefs -t ffs -s 4g -o version=2 "$artifacts/$base.img" "$root"
fi
