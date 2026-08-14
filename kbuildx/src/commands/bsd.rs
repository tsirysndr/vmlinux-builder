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
const BSD_BUILD_DISK_SIZE: u64 = 16 * 1024 * 1024 * 1024;
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
        .filter(|version| !version.is_empty())
        .unwrap_or("current");
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
    let machine_version = if os == BuildOs::Netbsd {
        "10.1"
    } else {
        version
    };
    let name = format!("kbuildx_{}_{}", os_name, safe_label(machine_version));
    let (sandbox, created) = match Sandbox::get(&name) {
        Ok(sandbox) => (sandbox, false),
        Err(_) => {
            println!(
                "{} {}",
                step("[1/4 SANDBOX]"),
                action("Creating persistent BSD build machine")
            );
            // Use the stable NetBSD 10.1 guest image, matching the package
            // repository used by the bsdkrun workflows. The requested version
            // still selects the source ref and artifact label below.
            let mut command = Command::new(bsdkrun_binary());
            let build_disk = std::env::current_dir()?.join(format!(".{}-build.img", name));
            if !build_disk.exists() {
                std::fs::OpenOptions::new()
                    .create(true)
                    .write(true)
                    .open(&build_disk)
                    .with_context(|| format!("creating {}", build_disk.display()))?
                    .set_len(BSD_BUILD_DISK_SIZE)
                    .with_context(|| format!("sizing {}", build_disk.display()))?;
            }
            command
                .arg(os_name)
                .arg("-d")
                .arg("--name")
                .arg(&name)
                .arg("--cpus")
                .arg(cpus.to_string())
                .arg("--mem")
                .arg(memory.to_string())
                .arg("--disk-size")
                .arg(BSD_DISK_SIZE)
                .arg("--volume")
                .arg(&name)
                .arg("--attach-disk")
                .arg(&build_disk)
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());
            if !(os == BuildOs::Netbsd && version == "current") {
                command.arg("--version").arg(machine_version);
            }
            let output = command.output().context("creating BSD build sandbox")?;
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
        BuildOs::Freebsd => runtime.run("/bin/sh", &["-s"], Some(FREEBSD_DEPS_SCRIPT), false),
        BuildOs::Netbsd => runtime.run(
            "/bin/sh",
            &["-s", "--", version],
            Some(NETBSD_DEPS_SCRIPT),
            false,
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

const NETBSD_DEPS_SCRIPT: &str = include_str!("../../scripts/netbsd-deps.sh");
const FREEBSD_DEPS_SCRIPT: &str = include_str!("../../scripts/freebsd-deps.sh");

fn default_bsd_ref(os: BuildOs, version: &str) -> String {
    match os {
        BuildOs::Freebsd if matches!(version, "current" | "main") => "main".to_string(),
        BuildOs::Freebsd => format!("releng/{}", version.trim_end_matches("-RELEASE")),
        BuildOs::Netbsd
            if matches!(version, "current" | "trunk") || version.starts_with("11.99.") =>
        {
            "trunk".to_string()
        }
        BuildOs::Netbsd => format!(
            "netbsd-{}",
            version
                .trim_start_matches("NetBSD-")
                .split(['.', '-'])
                .next()
                .unwrap_or(version)
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

const FREEBSD_BUILD_SCRIPT: &str = include_str!("../../scripts/freebsd-build.sh");
const NETBSD_BUILD_SCRIPT: &str = include_str!("../../scripts/netbsd-build.sh");

#[cfg(test)]
mod tests {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };

    use crate::cli::BuildOs;

    use super::{
        FREEBSD_BUILD_SCRIPT, FREEBSD_DEPS_SCRIPT, NETBSD_BUILD_SCRIPT, NETBSD_DEPS_SCRIPT,
        default_bsd_ref, safe_label,
    };

    #[test]
    fn freebsd_versions_map_to_releng_branches() {
        assert_eq!(default_bsd_ref(BuildOs::Freebsd, "15.1"), "releng/15.1");
        assert_eq!(default_bsd_ref(BuildOs::Freebsd, "current"), "main");
    }

    #[test]
    fn netbsd_versions_map_to_release_tags() {
        assert_eq!(default_bsd_ref(BuildOs::Netbsd, "10.1"), "netbsd-10");
        assert_eq!(default_bsd_ref(BuildOs::Netbsd, "current"), "trunk");
        assert_eq!(default_bsd_ref(BuildOs::Netbsd, "11.0"), "netbsd-11");
    }

    #[test]
    fn sandbox_names_are_safe() {
        assert_eq!(safe_label("15.1/TEST"), "15.1-TEST");
    }

    #[test]
    fn embedded_bsd_shell_scripts_are_valid() {
        for script in [
            FREEBSD_BUILD_SCRIPT,
            FREEBSD_DEPS_SCRIPT,
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
