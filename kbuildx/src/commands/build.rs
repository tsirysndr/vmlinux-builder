use anyhow::{Result, bail};
use bsdkrun_sdk::Sandbox;
use owo_colors::{OwoColorize, Rgb};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::{
    cli::{BuildArgs, BuildOs},
    config::KernelConfig,
    consts::KERNEL_REPO,
};

enum Runtime {
    Host,
    Sandbox(Sandbox),
}

impl Runtime {
    fn run(&self, program: &str, args: &[&str], stdin: Option<&str>, tty: bool) -> Result<i32> {
        match self {
            Self::Host => {
                let mut command = Command::new(program);
                command
                    .args(args)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .stdin(if stdin.is_some() {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    });
                let mut child = command.spawn()?;
                if let Some(input) = stdin {
                    child
                        .stdin
                        .take()
                        .expect("piped stdin must be available")
                        .write_all(input.as_bytes())?;
                }
                let status = child.wait()?;
                if !status.success() {
                    bail!(
                        "command failed (exit {}): {} {}",
                        status.code().unwrap_or(-1),
                        program,
                        args.join(" ")
                    );
                }
                Ok(status.code().unwrap_or_default())
            }
            Self::Sandbox(sandbox) => {
                let binary = std::env::var_os("BSDKRUN_BIN").unwrap_or_else(|| "bsdkrun".into());
                let mut command = Command::new(binary);
                command.arg("exec");
                if tty {
                    command.arg("-t");
                }
                command
                    .arg(sandbox.id())
                    .arg(program)
                    .args(args)
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .stdin(if stdin.is_some() {
                        Stdio::piped()
                    } else {
                        Stdio::null()
                    });
                let mut child = command.spawn()?;
                if let Some(input) = stdin {
                    child
                        .stdin
                        .take()
                        .expect("piped stdin must be available")
                        .write_all(input.as_bytes())?;
                }
                let status = child.wait()?;
                if !status.success() {
                    bail!(
                        "command failed (exit {}): bsdkrun exec {} {} {}",
                        status.code().unwrap_or(-1),
                        sandbox.id(),
                        program,
                        args.join(" ")
                    );
                }
                Ok(status.code().unwrap_or_default())
            }
        }
    }

    fn export_vmlinux(&self, version_label: &str) -> Result<()> {
        let Self::Sandbox(sandbox) = self else {
            return Ok(());
        };
        let filename = format!("vmlinux-{version_label}.{}", artifact_arch());
        let host_dir = std::env::current_dir()?.join("linux");
        std::fs::create_dir_all(&host_dir)?;
        for suffix in ["", ".sha256"] {
            let artifact = format!("{filename}{suffix}");
            export_sandbox_file(
                sandbox,
                &format!("/linux/{artifact}"),
                &host_dir.join(&artifact),
            )?;
        }
        println!(
            "{} {} {}",
            step_label("[EXPORT]"),
            success("Copied vmlinux artifacts to host:"),
            value(&host_dir.display().to_string())
        );
        Ok(())
    }
}

pub(crate) fn artifact_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "aarch64",
        "x86_64" => "x86_64",
        architecture => architecture,
    }
}

pub(crate) fn export_sandbox_file(
    sandbox: &Sandbox,
    guest_path: &str,
    host_path: &Path,
) -> Result<()> {
    let binary = std::env::var_os("BSDKRUN_BIN").unwrap_or_else(|| "bsdkrun".into());
    let temporary = temporary_artifact_path(host_path);
    let output = std::fs::File::create(&temporary)?;
    let status = Command::new(binary)
        .arg("exec")
        .arg(sandbox.id())
        .arg("cat")
        .arg(guest_path)
        .stdin(Stdio::null())
        .stdout(Stdio::from(output))
        .stderr(Stdio::inherit())
        .status()?;
    if !status.success() {
        let _ = std::fs::remove_file(&temporary);
        bail!(
            "failed to copy sandbox artifact {guest_path} (exit {})",
            status.code().unwrap_or(-1)
        );
    }
    std::fs::rename(&temporary, host_path)?;
    Ok(())
}

fn temporary_artifact_path(path: &Path) -> PathBuf {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("vmlinux");
    path.with_file_name(format!(".{filename}.part"))
}

#[cfg(target_os = "linux")]
fn command_exists(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(target_os = "linux")]
fn host_package_manager() -> Option<&'static str> {
    ["apk", "apt-get", "dnf"]
        .into_iter()
        .find(|program| command_exists(program))
}

fn step_label(label: &str) -> String {
    label.color(Rgb(125, 86, 244)).bold().to_string()
}

fn action(text: &str) -> String {
    text.color(Rgb(0, 215, 215)).to_string()
}

fn success(text: &str) -> String {
    text.color(Rgb(0, 215, 135)).to_string()
}

fn warning(text: &str) -> String {
    text.color(Rgb(255, 215, 95)).to_string()
}

fn value(text: &str) -> String {
    text.color(Rgb(175, 135, 255)).to_string()
}

fn muted(text: &str) -> String {
    text.color(Rgb(108, 112, 134)).to_string()
}

fn kernel_git_ref(version: &str) -> String {
    let version = version.strip_prefix('v').unwrap_or(version);
    if version.ends_with(".y") {
        format!("linux-{version}")
    } else {
        format!("v{version}")
    }
}

fn kernel_version_label(version: &str) -> String {
    version
        .strip_prefix('v')
        .unwrap_or(version)
        .strip_suffix(".y")
        .unwrap_or_else(|| version.strip_prefix('v').unwrap_or(version))
        .to_string()
}

fn safe_label(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn configured_kernel(args: &BuildArgs) -> Result<KernelConfig> {
    let mut config = KernelConfig::default();
    for setting in &args.set_config {
        let Some((name, raw_value)) = setting.split_once('=') else {
            bail!("invalid --set-config '{setting}'; expected NAME=y|m|n");
        };
        let name = name.strip_prefix("CONFIG_").unwrap_or(name);
        let value = match raw_value {
            "y" => crate::config::ConfigValue::Yes,
            "m" => crate::config::ConfigValue::Module,
            "n" => crate::config::ConfigValue::No,
            _ => bail!("invalid value in --set-config '{setting}'; expected y, m, or n"),
        };
        if !config.set(name, value) {
            bail!("unknown built-in kernel config option: {name}");
        }
    }
    Ok(config)
}

pub fn build_kernel(args: BuildArgs) -> Result<()> {
    if args.os != BuildOs::Linux {
        return crate::commands::bsd::build_bsd(args);
    }
    if args.bundle {
        bail!("--bundle is available only with --os freebsd or --os netbsd");
    }
    let repo = args.repo.as_str();
    let (git_ref, version_label) = if repo != KERNEL_REPO {
        let Some(branch) = args.branch.as_deref() else {
            bail!("--branch is required when --repo points to a custom repository");
        };
        let label = args
            .version_label
            .clone()
            .or_else(|| args.kernel_version.clone())
            .unwrap_or_else(|| safe_label(branch));
        (branch.to_string(), label)
    } else {
        let version = args
            .kernel_version
            .clone()
            .unwrap_or_else(|| fetch_last_version(repo));
        (kernel_git_ref(&version), kernel_version_label(&version))
    };
    println!(
        "{} Build {} @ {}",
        step_label("[BUILD]"),
        value(repo),
        muted(&git_ref)
    );

    #[cfg(not(target_os = "linux"))]
    if args.host {
        bail!("--host is supported only on Linux; this platform must use bsdkrun");
    }

    #[cfg(target_os = "linux")]
    let direct_host = if args.host {
        Some(
            host_package_manager()
                .ok_or_else(|| anyhow::anyhow!("--host requires one of: apk, apt-get, or dnf"))?,
        )
    } else if command_exists("apk") {
        Some("apk")
    } else {
        None
    };
    #[cfg(not(target_os = "linux"))]
    let direct_host: Option<&str> = None;

    let runtime = if let Some(package_manager) = direct_host {
        println!(
            "{} {} {}",
            step_label("[1/4 HOST]"),
            success("Linux host build enabled; package manager:"),
            value(package_manager)
        );
        Runtime::Host
    } else {
        Runtime::Sandbox(start_sandbox(args.cpus, args.memory, &args.disk_size)?)
    };
    install_deps(&runtime)?;
    prepare_build_disk(&runtime)?;
    sync_kernel(&runtime, repo, &git_ref)?;
    println!(
        "{} {}",
        step_label("[3/4 KERNEL]"),
        success("Kernel checkout is ready")
    );
    start_compilation(&runtime, &args, &version_label)?;
    runtime.export_vmlinux(&version_label)?;

    Ok(())
}

fn cache_versions() -> String {
    let git = Command::new("git")
        .args(&["ls-remote", "--tags", KERNEL_REPO])
        .output()
        .expect("Failed to execute git command");

    let output = String::from_utf8_lossy(&git.stdout);

    std::fs::write("/tmp/kernel_versions.txt", &*output)
        .expect("Failed to write to /tmp/kernel_versions.txt");

    output.to_string()
}

fn fetch_last_version(repo: &str) -> String {
    if Path::new("/tmp/kernel_versions.txt").exists() {
        // if file exists and is older than 24 hours, update it
        let metadata = std::fs::metadata("/tmp/kernel_versions.txt")
            .expect("Failed to get metadata for /tmp/kernel_versions.txt");
        let modified = metadata.modified().expect("Failed to get modified time");
        let now = std::time::SystemTime::now();

        if now.duration_since(modified).unwrap().as_secs() > 24 * 60 * 60 {
            cache_versions();
        }

        let output = std::fs::read_to_string("/tmp/kernel_versions.txt")
            .expect("Failed to read /tmp/kernel_versions.txt");

        let versions: Vec<&str> = output
            .lines()
            .filter(|line| !line.ends_with("^{}") && !line.contains("rc"))
            .filter_map(|line| line.split("refs/tags/v").nth(1))
            .collect();

        return versions.last().unwrap_or(&"latest").to_string();
    }

    let output = Command::new("git")
        .args(&["ls-remote", "--tags", repo])
        .output()
        .expect("Failed to execute git command");

    let output_str = String::from_utf8_lossy(&output.stdout);
    let versions: Vec<&str> = output_str
        .lines()
        .filter(|line| !line.ends_with("^{}") && !line.contains("rc"))
        .filter_map(|line| line.split("refs/tags/v").nth(1))
        .collect();

    versions.last().unwrap_or(&"latest").to_string()
}

const SANDBOX_ID: &str = "kbuildx_sandbox";

/// The persistent raw image attached to the Linux sandbox as virtio-blk. The
/// build runs on it (formatted ext4, mounted at /linux) instead of the
/// virtio-fs rootfs, whose per-file host round-trips dominate kernel-build
/// time on macOS.
fn build_disk_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join(format!(".{SANDBOX_ID}-build.img")))
}

fn create_sandbox(cpus: u32, memory: u32, disk_size: &str) -> Result<Sandbox> {
    use anyhow::Context;
    let build_disk = build_disk_path()?;
    if !build_disk.exists() {
        std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&build_disk)
            .with_context(|| format!("creating {}", build_disk.display()))?
            .set_len(crate::commands::bsd::parse_disk_size(disk_size)?)
            .with_context(|| format!("sizing {}", build_disk.display()))?;
    }
    let sandbox = Sandbox::linux("alpine:latest")
        .name(SANDBOX_ID)
        .cpus(cpus)
        .mem(memory)
        .attach_disk(build_disk.to_string_lossy())
        .create()
        .context("creating the Linux build sandbox (requires bsdkrun >= 0.9.0)")?;
    println!(
        "{} {} {}",
        step_label("[1/4 SANDBOX]"),
        success("Created new sandbox:"),
        success(&sandbox.id())
    );
    Ok(sandbox)
}

fn sandbox_info(name: &str) -> Result<Option<bsdkrun_sdk::SandboxInfo>> {
    Ok(Sandbox::list(true)?
        .into_iter()
        .find(|info| info.name.as_deref() == Some(name) || info.id == name))
}

fn start_sandbox(cpus: u32, memory: u32, disk_size: &str) -> Result<Sandbox> {
    let sandbox = match Sandbox::get(SANDBOX_ID) {
        Ok(sandbox) => sandbox,
        Err(s) => {
            eprintln!(
                "{} {} {}",
                step_label("[1/4 SANDBOX]"),
                warning("Existing sandbox unavailable:"),
                s
            );
            return create_sandbox(cpus, memory, disk_size);
        }
    };

    let info = sandbox_info(SANDBOX_ID)?;

    // A sandbox created before build disks existed keeps compiling over
    // virtio-fs. Its /linux checkout is only a cache, so recreate it around a
    // block device instead. bsdkrun records attached disks in the machine's
    // state dir as attached-disks.json.
    let has_build_disk = info
        .as_ref()
        .map(|info| {
            Path::new(&info.state_dir)
                .join("attached-disks.json")
                .exists()
        })
        .unwrap_or(false);
    if !has_build_disk {
        println!(
            "{} {}",
            step_label("[1/4 SANDBOX]"),
            warning(
                "Sandbox has no build disk; recreating it with one (the kernel checkout will be re-cloned)."
            )
        );
        sandbox.remove(true)?;
        return create_sandbox(cpus, memory, disk_size);
    }

    // Only cycle the sandbox when the requested resources differ from what it
    // already runs with — a restart drops the guest page cache and re-clones
    // nothing, but costs boot time on every build.
    let (running, resources_match) = info
        .map(|info| {
            (
                info.running,
                info.cpus == cpus && info.mem == u64::from(memory),
            )
        })
        .unwrap_or((false, false));
    if running && resources_match {
        println!(
            "{} {} {} ({} vCPU, {} MiB)",
            step_label("[1/4 SANDBOX]"),
            success("Reusing running sandbox:"),
            value(&sandbox.id()),
            cpus,
            memory
        );
        return Ok(sandbox);
    }

    println!(
        "{} {} {} ({} vCPU, {} MiB)",
        step_label("[1/4 SANDBOX]"),
        action("Configuring sandbox:"),
        value(&sandbox.id()),
        cpus,
        memory
    );
    if running {
        println!(
            "{} {}",
            step_label("[1/4 SANDBOX]"),
            action("Restarting sandbox to apply resources.")
        );
        sandbox.stop()?;
    }
    if !resources_match {
        sandbox.update().cpus(cpus).mem(memory).apply()?;
    }
    sandbox.start()?;

    Ok(sandbox)
}

fn install_deps(runtime: &Runtime) -> Result<()> {
    println!(
        "{} {}",
        step_label("[2/4 DEPS]"),
        action("Installing build dependencies")
    );
    let exit_code = runtime.run(
        "sh",
        &[
            "-c",
            r#"set -e
# Skip the package-manager round trip entirely once this dependency set is
# installed; bump the marker version whenever the lists below change.
marker=/var/lib/kbuildx/deps-v2
if [ -f "$marker" ]; then
    echo "Dependencies already installed"
    exit 0
fi
run_as_root=""
if [ "$(id -u)" -ne 0 ]; then
    if command -v sudo >/dev/null 2>&1; then
        run_as_root=sudo
    else
        echo "Host dependency installation requires root or sudo" >&2
        exit 1
    fi
fi

if command -v apk >/dev/null 2>&1; then
    $run_as_root apk add --no-cache git build-base flex bison ncurses-dev \
        openssl-dev gcc bc elfutils-dev pahole curl tar gzip u-boot-tools \
        mkinitfs bash perl python3 rsync cpio xz zstd findutils diffutils \
        linux-headers e2fsprogs
elif command -v apt-get >/dev/null 2>&1; then
    $run_as_root apt-get update
    $run_as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y \
        git build-essential flex bison libncurses-dev libssl-dev gcc bc \
        libelf-dev dwarves curl tar gzip u-boot-tools initramfs-tools bash \
        perl python3 rsync cpio xz-utils zstd findutils diffutils linux-libc-dev
elif command -v dnf >/dev/null 2>&1; then
    $run_as_root dnf install -y git gcc gcc-c++ make flex bison ncurses-devel \
        openssl-devel bc elfutils-libelf-devel dwarves curl tar gzip uboot-tools \
        dracut bash perl python3 rsync cpio xz zstd findutils diffutils \
        kernel-headers
else
    echo "No supported package manager found (apk, apt-get, or dnf)" >&2
    exit 1
fi
$run_as_root mkdir -p "$(dirname "$marker")"
$run_as_root touch "$marker""#,
        ],
        None,
        true,
    )?;

    println!(
        "{} {} {}",
        step_label("[2/4 DEPS]"),
        success("Dependencies installed; exit code:"),
        success(&exit_code.to_string())
    );

    Ok(())
}

/// Format (first use) and mount the sandbox's virtio-blk build disk at /linux,
/// so checkout and compilation run on ext4 with a guest page cache instead of
/// virtio-fs. Host builds have no attached disk and skip this entirely.
fn prepare_build_disk(runtime: &Runtime) -> Result<()> {
    let Runtime::Sandbox(_) = runtime else {
        return Ok(());
    };
    runtime.run(
        "sh",
        &[
            "-c",
            r#"set -e
disk=/dev/vda
if [ ! -b "$disk" ]; then
    echo "warning: no build disk attached; building on the virtio-fs root (slow)" >&2
    exit 0
fi
mkdir -p /linux
if mountpoint -q /linux; then
    exit 0
fi
if [ -z "$(blkid "$disk" 2>/dev/null)" ]; then
    echo "Formatting build disk $disk as ext4"
    mkfs.ext4 -q -L kbuildx "$disk"
fi
mount "$disk" /linux
echo "Build disk mounted at /linux""#,
        ],
        None,
        true,
    )?;
    println!(
        "{} {}",
        step_label("[2/4 DEPS]"),
        success("Build disk is ready")
    );
    Ok(())
}

fn sync_kernel(runtime: &Runtime, repo: &str, git_ref: &str) -> Result<()> {
    let kernel_label = step_label("[3/4 KERNEL]");
    let reuse_message = format!(
        "{} {}",
        kernel_label,
        action("Reusing existing Linux kernel checkout")
    );
    let remove_message = format!(
        "{} {}",
        kernel_label,
        warning("Removing incomplete Linux kernel checkout")
    );
    let clone_message = format!(
        "{} {}",
        kernel_label,
        action("Cloning Linux kernel checkout")
    );
    let current_tag_message = format!("{} {}", kernel_label, success("Current kernel tag:"));
    let current_commit_message = format!("{} Current commit: \x1b[38;2;0;215;215m", kernel_label);

    let exit_code = runtime.run(
        "sh",
        &[
            "-c",
            r#"
set -e
kernel_dir="${PWD%/}/linux"
if ! git config --global --get-all safe.directory | grep -Fxq "$kernel_dir"; then
    git config --global --add safe.directory "$kernel_dir"
fi
if git -C "$kernel_dir" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    printf '%s\n' "$3"
    git -C "$kernel_dir" fetch --force --depth 1 origin "$1"
    git -C "$kernel_dir" -c checkout.workers=1 checkout --force --detach FETCH_HEAD
else
    if [ -e "$kernel_dir" ]; then
        printf '%s\n' "$4"
        rm -rf -- "$kernel_dir"
    fi
    printf '%s\n' "$5"
    git -c checkout.workers=1 clone --depth 1 --branch "$1" "$2" "$kernel_dir"
fi
current_tag=$(git -C "$kernel_dir" describe --tags --exact-match HEAD 2>/dev/null || printf '%s' "$1")
printf '%s %s\n' "$6" "$current_tag"
current_commit=$(git -C "$kernel_dir" rev-parse --short HEAD)
printf '%s%s\033[0m\n' "$7" "$current_commit"
"#,
            "sh",
            git_ref,
            repo,
            &reuse_message,
            &remove_message,
            &clone_message,
            &current_tag_message,
            &current_commit_message,
        ],
        None,
        true,
    )?;

    println!(
        "{} {} {}",
        kernel_label,
        success("Kernel sync exit code:"),
        success(&exit_code.to_string())
    );

    Ok(())
}

fn start_compilation(runtime: &Runtime, args: &BuildArgs, version_label: &str) -> Result<()> {
    let kernel_config = configured_kernel(args)?.to_string();
    let defconfig = args.defconfig.as_deref().unwrap_or_default();
    let merge_config = args.merge_config.as_deref().unwrap_or_default();
    let uimage_name = args.uimage_name.as_deref().unwrap_or_default();
    let initrd = if args.initrd { "1" } else { "0" };
    let modules = if args.modules { "1" } else { "0" };
    let uimage = if args.uimage { "1" } else { "0" };
    println!(
        "{} {} {}",
        step_label("[4/4 BUILD]"),
        action("Building Linux kernel with config:"),
        value("default")
    );

    let exit_code = runtime.run(
        "sh",
        &[
            "-c",
            r#"set -e
kernel_dir="${PWD%/}/linux"
cd "$kernel_dir"

default_config=/tmp/kbuildx-default.config
cat > "$default_config"
version_label=$1
defconfig=$2
merge_config=$3
gen_initrd=$4
gen_modules=$5
gen_uimage=$6
uimage_arch=$7
uimage_os=$8
uimage_type=$9
uimage_comp=${10}
uimage_load=${11}
uimage_entry=${12}
uimage_name=${13}

root_run() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif command -v sudo >/dev/null 2>&1; then
        sudo "$@"
    else
        echo "This build step requires root or sudo: $*" >&2
        return 1
    fi
}

# Preserve a config stored in the checkout before mrproper removes .config.
merge_file=""
if [ -n "$merge_config" ]; then
    case "$merge_config" in
        http://*|https://*) ;;
        *)
            merge_file=/tmp/kbuildx-merge.config
            cp "$merge_config" "$merge_file"
            ;;
    esac
fi

rm -rf Documentation/Kbuild
make mrproper

if [ -n "$defconfig" ]; then
    printf '%s\n' "Using board defconfig $defconfig"
    make "$defconfig"
    cp .config board.config
    cp "$default_config" default.config
    scripts/kconfig/merge_config.sh -m default.config board.config
    printf '%s\n' '# CONFIG_WERROR is not set' >> .config
elif [ -n "$merge_config" ]; then
    cp "$default_config" .config
    case "$merge_config" in
        http://*|https://*) curl --fail --location "$merge_config" >> .config ;;
        *) cat "$merge_file" >> .config ;;
    esac
else
    cp "$default_config" .config
fi

make olddefconfig
make prepare

jobs=$(nproc)
make -j"$jobs" vmlinux </dev/null
arch=$(uname -m)
vmlinux_artifact="vmlinux-${version_label}.${arch}"
cp vmlinux "$vmlinux_artifact"
sha256sum "$vmlinux_artifact" > "$vmlinux_artifact.sha256"
printf 'vmlinux built: %s/%s\n' "$kernel_dir" "$vmlinux_artifact"

is_arm64=0
case "$arch" in aarch64|arm64) is_arm64=1 ;; esac

if [ "$is_arm64" = 1 ]; then
    make -j"$jobs" Image
    image_artifact="Image-${version_label}.${arch}"
    cp arch/arm64/boot/Image "$image_artifact"
    sha256sum "$image_artifact" > "$image_artifact.sha256"
    printf 'boot Image built: %s/%s\n' "$kernel_dir" "$image_artifact"
fi

if [ "$gen_uimage" = 1 ]; then
    if [ "$is_arm64" = 1 ]; then
        image_src=arch/arm64/boot/Image
        if [ "$uimage_comp" = gzip ]; then
            gzip -9 -k -f "$image_src"
            image_src="$image_src.gz"
        fi
        krel=$(make -s kernelrelease)
        [ -n "$uimage_name" ] || uimage_name="Linux $krel"
        mkimage -A "$uimage_arch" -O "$uimage_os" -T "$uimage_type" \
            -C "$uimage_comp" -a "$uimage_load" -e "$uimage_entry" \
            -n "$uimage_name" -d "$image_src" uImage
        sha256sum uImage > uImage.sha256
        printf 'uImage built: %s/uImage\n' "$kernel_dir"
    else
        printf '%s\n' "--uimage requested on $arch; skipping"
    fi
fi

has_modules=0
grep -q '^CONFIG_MODULES=y$' .config && has_modules=1
krel=$(make -s kernelrelease)

if [ "$gen_initrd" = 1 ] || [ "$gen_modules" = 1 ]; then
    if [ "$has_modules" = 1 ]; then
        make -j"$jobs" modules
        rm -rf modules-out
        make modules_install INSTALL_MOD_PATH="$kernel_dir/modules-out"
    fi
fi

if [ "$gen_initrd" = 1 ]; then
    initrd_artifact="initrd.img-${krel}"
    if [ "$has_modules" = 1 ]; then
        root_run make modules_install
    else
        root_run mkdir -p "/lib/modules/$krel"
    fi
    if command -v mkinitramfs >/dev/null 2>&1; then
        root_run mkinitramfs -o "$initrd_artifact" "$krel"
    elif command -v mkinitfs >/dev/null 2>&1; then
        root_run mkinitfs -o "$initrd_artifact" "$krel"
    elif command -v dracut >/dev/null 2>&1; then
        root_run dracut --force "$initrd_artifact" "$krel"
    else
        echo 'No initramfs generator is installed' >&2
        exit 1
    fi
    sha256sum "$initrd_artifact" > "$initrd_artifact.sha256"
    printf 'initrd built: %s/%s\n' "$kernel_dir" "$initrd_artifact"
    if [ "$is_arm64" = 1 ]; then
        mkimage -A arm64 -O linux -T ramdisk -C gzip -n "uInitrd $krel" \
            -d "$initrd_artifact" uInitrd
        sha256sum uInitrd > uInitrd.sha256
        printf 'uInitrd built: %s/uInitrd\n' "$kernel_dir"
    fi
fi

if [ "$gen_modules" = 1 ]; then
    if [ "$has_modules" = 1 ]; then
        modules_artifact="modules-${krel}.tar.gz"
        tar --exclude=build --exclude=source -czf "$modules_artifact" \
            -C modules-out lib/modules
        sha256sum "$modules_artifact" > "$modules_artifact.sha256"
        printf 'modules archived: %s/%s\n' "$kernel_dir" "$modules_artifact"
    else
        printf '%s\n' '--modules requested but CONFIG_MODULES is disabled; skipping'
    fi
fi
            "#,
            "sh",
            version_label,
            defconfig,
            merge_config,
            initrd,
            modules,
            uimage,
            &args.uimage_arch,
            &args.uimage_os,
            &args.uimage_type,
            &args.uimage_comp,
            &args.uimage_load,
            &args.uimage_entry,
            uimage_name,
        ],
        Some(&kernel_config),
        false,
    )?;

    println!(
        "{} {} {}",
        step_label("[4/4 BUILD]"),
        success("Kernel build exit code:"),
        success(&exit_code.to_string())
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{kernel_git_ref, temporary_artifact_path};

    #[test]
    fn kernel_version_is_normalized_to_a_tag_ref() {
        assert_eq!(kernel_git_ref("7.1.8"), "v7.1.8");
        assert_eq!(kernel_git_ref("v7.1.8"), "v7.1.8");
        assert_eq!(kernel_git_ref("6.6.y"), "linux-6.6.y");
        assert_eq!(kernel_git_ref("v6.6.y"), "linux-6.6.y");
    }

    #[test]
    fn sandbox_exports_use_a_hidden_partial_file() {
        assert_eq!(
            temporary_artifact_path(Path::new("linux/vmlinux-7.1.8.x86_64")),
            Path::new("linux/.vmlinux-7.1.8.x86_64.part")
        );
    }
}
