use anyhow::{Result, bail};
use bsdkrun_sdk::Sandbox;
use owo_colors::{OwoColorize, Rgb};
use std::path::Path;
use std::process::Command;

use crate::{cli::BuildArgs, config::KernelConfig, consts::KERNEL_REPO};

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

pub fn build_kernel(args: BuildArgs) -> Result<()> {
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

    let sbx = start_sandbox(args.cpus, args.memory)?;
    install_deps(&sbx)?;
    sync_kernel(&sbx, repo, &git_ref)?;
    println!(
        "{} {}",
        step_label("[3/4 KERNEL]"),
        success("Kernel checkout is ready")
    );
    start_compilation(&sbx, &args, &version_label)?;

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

fn start_sandbox(cpus: u32, memory: u32) -> Result<Sandbox> {
    const SANDBOX_ID: &str = "kbuildx_sandbox";
    let sandbox = Sandbox::get(SANDBOX_ID);

    if let Err(s) = sandbox {
        eprintln!(
            "{} {} {}",
            step_label("[1/4 SANDBOX]"),
            warning("Existing sandbox unavailable:"),
            s
        );
        let sandbox = Sandbox::linux("alpine:latest")
            .name(SANDBOX_ID)
            .cpus(cpus)
            .mem(memory)
            .create()?;
        println!(
            "{} {} {}",
            step_label("[1/4 SANDBOX]"),
            success("Created new sandbox:"),
            success(&sandbox.id())
        );
        return Ok(sandbox);
    }

    let sandbox = sandbox.unwrap();
    println!(
        "{} {} {} ({} vCPU, {} MiB)",
        step_label("[1/4 SANDBOX]"),
        action("Configuring sandbox:"),
        value(&sandbox.id()),
        cpus,
        memory
    );
    if sandbox.is_running()? {
        println!(
            "{} {}",
            step_label("[1/4 SANDBOX]"),
            action("Restarting sandbox to apply resources.")
        );
        sandbox.stop()?;
    }
    sandbox.update().cpus(cpus).mem(memory).apply()?;
    sandbox.start()?;

    Ok(sandbox)
}

fn install_deps(sbx: &Sandbox) -> Result<()> {
    println!(
        "{} {}",
        step_label("[2/4 DEPS]"),
        action("Installing build dependencies")
    );
    let result = sbx
        .command("sh")
        .args([
            "-c",
            r#"set -e
apk add --no-cache git build-base flex bison ncurses-dev openssl-dev gcc bc \
    elfutils-dev pahole curl tar gzip u-boot-tools mkinitfs bash perl python3 \
    rsync cpio xz zstd findutils"#,
        ])
        .stdout(std::io::stdout())
        .stderr(std::io::stderr())
        .tty(true)
        .run()?
        .ok_or_err()?;

    println!(
        "{} {} {}",
        step_label("[2/4 DEPS]"),
        success("Dependencies installed; exit code:"),
        success(&result.exit_code.to_string())
    );

    Ok(())
}

fn sync_kernel(sbx: &Sandbox, repo: &str, git_ref: &str) -> Result<()> {
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

    let result = sbx
        .command("sh")
        .args([
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
        ])
        .stdout(std::io::stdout())
        .stderr(std::io::stderr())
        .tty(true)
        .run()?
        .ok_or_err()?;

    println!(
        "{} {} {}",
        kernel_label,
        success("Kernel sync exit code:"),
        success(&result.exit_code.to_string())
    );

    Ok(())
}

fn start_compilation(sbx: &Sandbox, args: &BuildArgs, version_label: &str) -> Result<()> {
    let kernel_config = KernelConfig::default().to_string();
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

    let result = sbx
        .command("sh")
        .args([
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
        make modules_install
    else
        mkdir -p "/lib/modules/$krel"
    fi
    if command -v mkinitramfs >/dev/null 2>&1; then
        mkinitramfs -o "$initrd_artifact" "$krel"
    elif command -v mkinitfs >/dev/null 2>&1; then
        mkinitfs -o "$initrd_artifact" "$krel"
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
        ])
        .stdin(&kernel_config)
        .stdout(std::io::stdout())
        .stderr(std::io::stderr())
        .tty(true)
        .run()?
        .ok_or_err()?;

    println!(
        "{} {} {}",
        step_label("[4/4 BUILD]"),
        success("Kernel build exit code:"),
        success(&result.exit_code.to_string())
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::kernel_git_ref;

    #[test]
    fn kernel_version_is_normalized_to_a_tag_ref() {
        assert_eq!(kernel_git_ref("7.1.8"), "v7.1.8");
        assert_eq!(kernel_git_ref("v7.1.8"), "v7.1.8");
        assert_eq!(kernel_git_ref("6.6.y"), "linux-6.6.y");
        assert_eq!(kernel_git_ref("v6.6.y"), "linux-6.6.y");
    }
}
