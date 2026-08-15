#!/bin/sh
set -eu

version=$1; repo=$2; ref=$3; label=$4; requested_config=$5; bundle=$6
export PATH=/usr/pkg/gcc15/bin:/usr/pkg/bin:/sbin:/usr/sbin:/bin:/usr/bin
export LD_LIBRARY_PATH=/usr/pkg/gcc15/lib:/usr/pkg/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}
mkdir -p /tmp/netbsd-bin
for tool in as ld ar ranlib nm strip; do
    target=$(command -v "x86_64--netbsd-$tool" 2>/dev/null || true)
    if [ -z "$target" ] && [ -x "/usr/pkg/bin/$tool" ]; then
        target="/usr/pkg/bin/$tool"
    fi
    if [ -z "$target" ]; then
        # Exact or cross-prefixed names only: "*$tool" is too loose — git's
        # "scalar" matches "*ar" and once ended up as HOST_AR.
        target=$(find /usr/pkg -type f \( -name "$tool" -o -name "*-$tool" \) -perm -111 2>/dev/null | head -n 1 || true)
    fi
    # Only install tools that can actually run; a binary with a missing
    # shared library (or a false find match) would poison the whole build.
    if [ -n "$target" ] && "$target" --version >/dev/null 2>&1; then
        ln -sf "$target" "/tmp/netbsd-bin/$tool"
    fi
done

export PATH=/tmp/netbsd-bin:$PATH

# Repair /usr/lib/libgnuctf.so.2 only if a real library file exists somewhere;
# a dangling symlink would leave /usr/bin/ld unrunnable while looking "fixed".
rm -f /usr/lib/libgnuctf.so.2 2>/dev/null || true
gnuctf=$(find /usr/lib /lib /usr/pkg -name 'libgnuctf.so*' -type f 2>/dev/null | head -n 1 || true)
if [ -n "$gnuctf" ]; then
    ln -sf "$gnuctf" /usr/lib/libgnuctf.so.2 || true
fi

# pkgsrc gcc15 is built --with-ld=/usr/bin/ld, so -B alone cannot redirect the
# linker. If the base ld cannot even start (missing libgnuctf.so.2 on trimmed
# images), route collect2 to pkgsrc's ld via -fuse-ld=bfd, which ignores the
# configured DEFAULT_LINKER and searches the -B prefixes for "ld.bfd".
cc_flags='-B/usr/pkg/bin/'
if ! /usr/bin/ld --version >/dev/null 2>&1; then
    echo "WARNING: /usr/bin/ld is not runnable:"
    ldd /usr/bin/ld || true
    if [ -e /tmp/netbsd-bin/ld ]; then
        ln -sf "$(readlink /tmp/netbsd-bin/ld)" /tmp/netbsd-bin/ld.bfd
        cc_flags='-B/usr/pkg/bin/ -B/tmp/netbsd-bin/ -fuse-ld=bfd'
        echo "Using pkgsrc ld via -fuse-ld=bfd: $(readlink /tmp/netbsd-bin/ld)"
    else
        echo "ERROR: no usable linker found (base ld broken, no pkgsrc ld)" >&2
        exit 1
    fi
fi

export CC="gcc $cc_flags"
export HOST_CC="gcc $cc_flags"

printf 'int main(void){return 0;}\n' > /tmp/cc-smoke.c
if ! $CC -O /tmp/cc-smoke.c -o /tmp/cc-smoke; then
    echo "ERROR: host compiler smoke test failed (CC=$CC)" >&2
    ldd /usr/bin/ld || true
    ls -l /usr/lib/libgnuctf* /usr/pkg/bin 2>/dev/null || true
    exit 1
fi
rm -f /tmp/cc-smoke.c /tmp/cc-smoke
work=/root/kbuildx
src=$work/netbsd-src
obj=$work/netbsd-obj
tools=$work/netbsd-tools
dest=$work/netbsd-rootfs
artifacts=$work/artifacts
mkdir -p "$work" "$artifacts"
if git -C "$src" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$src" fetch --force --depth 1 origin "$ref"
    git -C "$src" checkout --force --detach FETCH_HEAD
else
    rm -rf "$src"
    git clone --depth 1 --branch "$ref" "$repo" "$src"
fi
machine=$(uname -m)
case "$machine" in amd64|x86_64) artifact_arch=x86_64 ;; *) artifact_arch=aarch64 ;; esac
config=$requested_config
if [ -z "$config" ]; then
    if [ "$bundle" = 1 ] && find "$src/sys/arch" -path '*/conf/MICROVM' | grep -q .; then
        config=MICROVM
    elif find "$src/sys/arch" -path '*/conf/GENERIC64' | grep -q . && [ "$artifact_arch" = aarch64 ]; then
        config=GENERIC64
    else
        config=GENERIC
    fi
fi
rm -rf "$obj" "$tools"
jobs=$(sysctl -n hw.ncpu)
cd "$src"
./build.sh -U -u -j"$jobs" -O "$obj" -T "$tools" tools
./build.sh -U -u -j"$jobs" -O "$obj" -T "$tools" kernel="$config"
kernel=$(find "$obj" -type f -path "*/compile/$config/netbsd" | head -n 1)
test -n "$kernel" -a -f "$kernel"
base="netbsd-${label}.${artifact_arch}"
cp "$kernel" "$artifacts/$base.kernel"
if [ "$bundle" = 1 ]; then
    rm -rf "$dest"; mkdir -p "$dest"
    ./build.sh -U -u -j"$jobs" -O "$obj" -T "$tools" -D "$dest" distribution
    mkdir -p "$dest/usr/local/sbin" "$dest/etc/rc.d"
    cp /usr/local/sbin/bsdkrun-agent "$dest/usr/local/sbin/bsdkrun-agent"
    chmod 755 "$dest/usr/local/sbin/bsdkrun-agent"
    cat > "$dest/etc/rc.d/bsdkrun_agent" <<'RC'
#!/bin/sh
# PROVIDE: bsdkrun_agent
# REQUIRE: NETWORKING
. /etc/rc.subr
name=bsdkrun_agent
rcvar=$name
command=/usr/local/sbin/bsdkrun-agent
pidfile=/var/run/bsdkrun_agent.pid
start_cmd=agent_start
agent_start() { ${command} & echo $! > ${pidfile}; }
load_rc_config $name
run_rc_command "$1"
RC
    chmod 555 "$dest/etc/rc.d/bsdkrun_agent"
    printf '\nbsdkrun_agent=YES\nrc_configured=YES\ndhcpcd=YES\n' >> "$dest/etc/rc.conf"
    printf '/dev/ld0a / ffs rw 1 1\nptyfs /dev/pts ptyfs rw 0 0\n' > "$dest/etc/fstab"
    (cd "$dest/dev" && sh ./MAKEDEV all)
    makefs -t ffs -s 4g "$artifacts/$base.img" "$dest"
fi
