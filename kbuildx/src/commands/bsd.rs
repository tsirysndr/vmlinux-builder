use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use bsdkrun_sdk::Sandbox;
use owo_colors::{OwoColorize, Rgb};

use crate::{
    cli::{BuildArgs, BuildOs},
    commands::build::{artifact_arch, export_sandbox_file},
    consts::KERNEL_REPO,
};

const FREEBSD_REPO: &str = "https://git.FreeBSD.org/src.git";
const NETBSD_REPO: &str = "https://github.com/NetBSD/src.git";
const BSD_DISK_SIZE: &str = "40G";
const BSD_AGENT_TIMEOUT: Duration = Duration::from_secs(180);

fn step(label: &str) -> String {
    label.color(Rgb(125, 86, 244)).bold().to_string()
}

fn action(message: &str) -> String {
    message.color(Rgb(0, 215, 215)).to_string()
}

fn success(message: &str) -> String {
    message.color(Rgb(0, 215, 135)).to_string()
}

fn value(message: &str) -> String {
    message.color(Rgb(175, 135, 255)).to_string()
}

fn bsdkrun_binary() -> std::ffi::OsString {
    std::env::var_os("BSDKRUN_BIN").unwrap_or_else(|| "bsdkrun".into())
}

struct BsdRuntime {
    sandbox: Sandbox,
}

impl BsdRuntime {
    fn run(&self, program: &str, args: &[&str], input: Option<&str>, tty: bool) -> Result<()> {
        let mut command = Command::new(bsdkrun_binary());
        command.arg("exec");
        if tty {
            command.arg("-t");
        }
        command
            .arg(self.sandbox.id())
            .arg(program)
            .args(args)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            });
        let mut child = command.spawn().context("starting bsdkrun exec")?;
        if let Some(input) = input {
            child
                .stdin
                .take()
                .expect("piped stdin must be available")
                .write_all(input.as_bytes())?;
        }
        let status = child.wait()?;
        if !status.success() {
            bail!(
                "bsdkrun exec failed (exit {}): {program} {}",
                status.code().unwrap_or(-1),
                args.join(" ")
            );
        }
        Ok(())
    }
}

pub fn build_bsd(args: BuildArgs) -> Result<()> {
    if args.host {
        bail!("--host supports Linux only; BSD builds require bsdkrun");
    }
    if args.merge_config.is_some()
        || args.initrd
        || args.modules
        || args.uimage
        || !args.set_config.is_empty()
    {
        bail!("Linux config, module, initrd, and uImage options cannot be used with BSD builds");
    }
    let version = args
        .kernel_version
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("a FreeBSD or NetBSD version is required"))?;
    let os = args.os;
    let os_name = match os {
        BuildOs::Freebsd => "freebsd",
        BuildOs::Netbsd => "netbsd",
        BuildOs::Linux => unreachable!(),
    };
    let repo = if args.repo == KERNEL_REPO {
        match os {
            BuildOs::Freebsd => FREEBSD_REPO,
            BuildOs::Netbsd => NETBSD_REPO,
            BuildOs::Linux => unreachable!(),
        }
    } else {
        &args.repo
    };
    let git_ref = args
        .branch
        .clone()
        .unwrap_or_else(|| default_bsd_ref(os, version));
    let label = args
        .version_label
        .clone()
        .unwrap_or_else(|| safe_label(version));

    println!(
        "{} {} {} {} {}",
        step("[BSD BUILD]"),
        action("Building"),
        value(os_name),
        action("version"),
        value(version)
    );
    let runtime = start_bsd_sandbox(os, version, args.cpus, args.memory)?;
    install_bsd_dependencies(&runtime, os, version)?;
    build_bsd_kernel(
        &runtime,
        os,
        version,
        repo,
        &git_ref,
        &label,
        args.defconfig.as_deref(),
        args.bundle,
    )?;
    export_bsd_artifacts(&runtime.sandbox, os_name, &label, args.bundle)?;
    Ok(())
}

fn start_bsd_sandbox(os: BuildOs, version: &str, cpus: u32, memory: u32) -> Result<BsdRuntime> {
    let os_name = match os {
        BuildOs::Freebsd => "freebsd",
        BuildOs::Netbsd => "netbsd",
        BuildOs::Linux => unreachable!(),
    };
    let name = format!("kbuildx_{}_{}", os_name, safe_label(version));
    let (sandbox, created) = match Sandbox::get(&name) {
        Ok(sandbox) => (sandbox, false),
        Err(_) => {
            println!(
                "{} {}",
                step("[1/4 SANDBOX]"),
                action("Creating persistent BSD build machine")
            );
            let output = Command::new(bsdkrun_binary())
                .arg(os_name)
                .arg("-d")
                .arg("--name")
                .arg(&name)
                .arg("--version")
                .arg(version)
                .arg("--cpus")
                .arg(cpus.to_string())
                .arg("--mem")
                .arg(memory.to_string())
                .arg("--disk-size")
                .arg(BSD_DISK_SIZE)
                .arg("--volume")
                .arg(&name)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .output()
                .context("creating BSD build sandbox")?;
            if !output.status.success() {
                bail!("unable to create {os_name} build sandbox");
            }
            let output_text = String::from_utf8_lossy(&output.stdout);
            let id = output_text
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.trim().to_string())
                .ok_or_else(|| anyhow::anyhow!("bsdkrun did not return a sandbox id"))?;
            (Sandbox::get(&id)?, true)
        }
    };
    if !created {
        if sandbox.is_running()? {
            sandbox.stop()?;
        }
        sandbox.update().cpus(cpus).mem(memory).apply()?;
        sandbox.start()?;
    }
    wait_for_bsd_agent(&sandbox)?;
    println!(
        "{} {} {}",
        step("[1/4 SANDBOX]"),
        success("BSD build machine ready:"),
        value(sandbox.id())
    );
    Ok(BsdRuntime { sandbox })
}

fn wait_for_bsd_agent(sandbox: &Sandbox) -> Result<()> {
    println!(
        "{} {}",
        step("[1/4 SANDBOX]"),
        action("Waiting for BSD exec agent")
    );
    let started = Instant::now();
    loop {
        let status = Command::new(bsdkrun_binary())
            .args(["exec", sandbox.id(), "/usr/bin/true"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .context("checking BSD exec agent readiness")?;
        if status.success() {
            return Ok(());
        }
        if !sandbox.is_running()? {
            bail!(
                "BSD build machine {} stopped before its exec agent became ready; inspect `bsdkrun logs --boot {}`",
                sandbox.id(),
                sandbox.id()
            );
        }
        if started.elapsed() >= BSD_AGENT_TIMEOUT {
            bail!(
                "timed out after {}s waiting for the BSD exec agent on {}; inspect `bsdkrun logs --boot {}`",
                BSD_AGENT_TIMEOUT.as_secs(),
                sandbox.id(),
                sandbox.id()
            );
        }
        thread::sleep(Duration::from_secs(1));
    }
}

fn install_bsd_dependencies(runtime: &BsdRuntime, os: BuildOs, version: &str) -> Result<()> {
    println!(
        "{} {}",
        step("[2/4 DEPS]"),
        action("Installing BSD build dependencies")
    );
    match os {
        BuildOs::Freebsd => runtime.run(
            "/bin/sh",
            &[
                "-c",
                "env ASSUME_ALWAYS_YES=yes pkg install -y git ca_root_nss",
            ],
            None,
            true,
        ),
        BuildOs::Netbsd => runtime.run(
            "/bin/sh",
            &["-s", "--", version],
            Some(NETBSD_DEPS_SCRIPT),
            true,
        ),
        BuildOs::Linux => unreachable!(),
    }
}

#[allow(clippy::too_many_arguments)]
fn build_bsd_kernel(
    runtime: &BsdRuntime,
    os: BuildOs,
    version: &str,
    repo: &str,
    git_ref: &str,
    label: &str,
    config: Option<&str>,
    bundle: bool,
) -> Result<()> {
    println!(
        "{} {} {}",
        step("[3/4 SOURCE]"),
        action("Syncing source ref:"),
        value(git_ref)
    );
    let script = match os {
        BuildOs::Freebsd => FREEBSD_BUILD_SCRIPT,
        BuildOs::Netbsd => NETBSD_BUILD_SCRIPT,
        BuildOs::Linux => unreachable!(),
    };
    runtime.run(
        "/bin/sh",
        &[
            "-s",
            "--",
            version,
            repo,
            git_ref,
            label,
            config.unwrap_or_default(),
            if bundle { "1" } else { "0" },
        ],
        Some(script),
        false,
    )
}

fn export_bsd_artifacts(sandbox: &Sandbox, os: &str, label: &str, bundle: bool) -> Result<()> {
    let arch = artifact_arch();
    let base = format!("{os}-{label}.{arch}");
    let host_dir = std::env::current_dir()?.join(os);
    std::fs::create_dir_all(&host_dir)?;
    let kernel = format!("{base}.kernel");
    let mut artifacts = vec![kernel.clone()];
    if bundle {
        artifacts.push(format!("{base}.img"));
    }
    for artifact in artifacts {
        export_sandbox_file(
            sandbox,
            &format!("/root/kbuildx/artifacts/{artifact}"),
            &host_dir.join(&artifact),
        )?;
    }
    write_host_checksum(&host_dir.join(kernel))?;
    if bundle {
        compress_host_image(os, arch, &host_dir.join(format!("{base}.img")))?;
    }
    println!(
        "{} {} {}",
        step("[4/4 EXPORT]"),
        success("Copied BSD artifacts to host:"),
        value(&host_dir.display().to_string())
    );
    Ok(())
}

fn compress_host_image(os: &str, arch: &str, image: &Path) -> Result<()> {
    println!(
        "{} {} {}",
        step("[4/4 EXPORT]"),
        action("Compressing rootfs on host:"),
        value(&image.display().to_string())
    );
    let use_xz = os == "freebsd" && arch == "aarch64";
    let (program, arguments, suffix) = if use_xz {
        ("xz", &["-T0", "-6", "-f"][..], ".xz")
    } else {
        ("gzip", &["-9", "-f"][..], ".gz")
    };
    let status = Command::new(program).args(arguments).arg(image).status()?;
    if !status.success() {
        bail!(
            "host {program} failed with exit {}",
            status.code().unwrap_or(-1)
        );
    }
    let compressed = Path::new(&format!("{}{suffix}", image.display())).to_path_buf();
    write_host_checksum(&compressed)
}

fn write_host_checksum(path: &Path) -> Result<()> {
    let directory = path.parent().unwrap_or_else(|| Path::new("."));
    let filename = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("artifact has no filename: {}", path.display()))?;
    let checksum = if cfg!(target_os = "macos") {
        Command::new("shasum")
            .args(["-a", "256"])
            .arg(filename)
            .current_dir(directory)
            .output()?
    } else {
        Command::new("sha256sum")
            .arg(filename)
            .current_dir(directory)
            .output()?
    };
    if !checksum.status.success() {
        bail!("unable to checksum compressed BSD rootfs");
    }
    std::fs::write(format!("{}.sha256", path.display()), checksum.stdout)?;
    Ok(())
}

const NETBSD_DEPS_SCRIPT: &str = r#"set -eu
version=$1
export PATH=/usr/pkg/bin:/sbin:/usr/sbin:/bin:/usr/bin
case "$(uname -m)" in
    amd64|x86_64) pkg_arch=x86_64 ;;
    *) pkg_arch=aarch64 ;;
esac
case "$version" in
    current|trunk) pkg_release=10.1 ;;
    *) pkg_release=${version%%-*} ;;
esac
mkdir -p /usr/pkg/etc/pkgin
printf 'https://cdn.NetBSD.org/pub/pkgsrc/packages/NetBSD/%s/%s/All\n' \
    "$pkg_arch" "$pkg_release" > /usr/pkg/etc/pkgin/repositories.conf
pkgin -y update
pkgin -y install git-base mozilla-rootcerts-openssl || pkgin -y install git
"#;

fn default_bsd_ref(os: BuildOs, version: &str) -> String {
    match os {
        BuildOs::Freebsd if matches!(version, "current" | "main") => "main".to_string(),
        BuildOs::Freebsd => format!("releng/{}", version.trim_end_matches("-RELEASE")),
        BuildOs::Netbsd if matches!(version, "current" | "trunk") => "trunk".to_string(),
        BuildOs::Netbsd => format!(
            "netbsd-{}-RELEASE",
            version
                .trim_start_matches("NetBSD-")
                .replace(['.', '-'], "-")
        ),
        BuildOs::Linux => unreachable!(),
    }
}

fn safe_label(input: &str) -> String {
    input
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-') {
                character
            } else {
                '-'
            }
        })
        .collect()
}

const FREEBSD_BUILD_SCRIPT: &str = r#"set -eu
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
"#;

const NETBSD_BUILD_SCRIPT: &str = r#"set -eu
version=$1; repo=$2; ref=$3; label=$4; requested_config=$5; bundle=$6
export PATH=/usr/pkg/bin:/sbin:/usr/sbin:/bin:/usr/bin
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
"#;

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    use crate::cli::BuildOs;

    use super::{
        FREEBSD_BUILD_SCRIPT, NETBSD_BUILD_SCRIPT, NETBSD_DEPS_SCRIPT, default_bsd_ref, safe_label,
    };

    #[test]
    fn freebsd_versions_map_to_releng_branches() {
        assert_eq!(default_bsd_ref(BuildOs::Freebsd, "15.1"), "releng/15.1");
        assert_eq!(default_bsd_ref(BuildOs::Freebsd, "current"), "main");
    }

    #[test]
    fn netbsd_versions_map_to_release_tags() {
        assert_eq!(
            default_bsd_ref(BuildOs::Netbsd, "10.1"),
            "netbsd-10-1-RELEASE"
        );
        assert_eq!(default_bsd_ref(BuildOs::Netbsd, "current"), "trunk");
    }

    #[test]
    fn sandbox_names_are_safe() {
        assert_eq!(safe_label("15.1/TEST"), "15.1-TEST");
    }

    #[test]
    fn embedded_bsd_shell_scripts_are_valid() {
        for script in [
            FREEBSD_BUILD_SCRIPT,
            NETBSD_BUILD_SCRIPT,
            NETBSD_DEPS_SCRIPT,
        ] {
            let mut child = Command::new("sh")
                .arg("-n")
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(script.as_bytes())
                .unwrap();
            assert!(child.wait().unwrap().success());
        }
    }
}
