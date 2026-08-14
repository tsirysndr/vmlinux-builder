use anyhow::Result;
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
    if version.starts_with('v') {
        version.to_string()
    } else {
        format!("v{version}")
    }
}

pub fn build_kernel(args: BuildArgs) -> Result<()> {
    let repo = args.repo.as_str();
    let version = args.version.unwrap_or_else(|| fetch_last_version(repo));
    println!(
        "{} Build {} {}",
        step_label("[BUILD]"),
        value(repo),
        muted(&version)
    );

    let sbx = start_sandbox(args.cpus, args.memory)?;
    install_deps(&sbx)?;
    sync_kernel(&sbx, repo, &version)?;
    println!(
        "{} {}",
        step_label("[3/3 KERNEL]"),
        success("Kernel checkout is ready")
    );
    start_compilation(&sbx)?;

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
            step_label("[1/3 SANDBOX]"),
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
            step_label("[1/3 SANDBOX]"),
            success("Created new sandbox:"),
            success(&sandbox.id())
        );
        return Ok(sandbox);
    }

    let sandbox = sandbox.unwrap();
    println!(
        "{} {} {} ({} vCPU, {} MiB)",
        step_label("[1/3 SANDBOX]"),
        action("Configuring sandbox:"),
        value(&sandbox.id()),
        cpus,
        memory
    );
    if sandbox.is_running()? {
        println!(
            "{} {}",
            step_label("[1/3 SANDBOX]"),
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
        step_label("[2/3 DEPS]"),
        action("Installing build dependencies")
    );
    let result = sbx
        .command("apk")
        .args([
            "add",
            "git",
            "build-base",
            "flex",
            "bison",
            "ncurses-dev",
            "openssl-dev",
            "gcc",
            "bc",
            "elfutils-dev",
            "pahole",
        ])
        .stdout(std::io::stdout())
        .stderr(std::io::stderr())
        .tty(true)
        .run()?;

    println!(
        "{} {} {}",
        step_label("[2/3 DEPS]"),
        success("Dependencies installed; exit code:"),
        success(&result.exit_code.to_string())
    );

    Ok(())
}

fn sync_kernel(sbx: &Sandbox, repo: &str, version: &str) -> Result<()> {
    let git_ref = kernel_git_ref(version);
    let kernel_label = step_label("[3/3 KERNEL]");
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
    git -C "$kernel_dir" fetch --force --depth 1 origin "refs/tags/$1:refs/tags/$1"
    git -C "$kernel_dir" -c checkout.workers=1 checkout --force --detach "refs/tags/$1"
else
    if [ -e "$kernel_dir" ]; then
        printf '%s\n' "$4"
        rm -rf -- "$kernel_dir"
    fi
    printf '%s\n' "$5"
    git -c checkout.workers=1 clone --depth 1 --branch "$1" "$2" "$kernel_dir"
fi
current_tag=$(git -C "$kernel_dir" describe --tags --exact-match HEAD)
printf '%s %s\n' "$6" "$current_tag"
current_commit=$(git -C "$kernel_dir" rev-parse --short HEAD)
printf '%s%s\033[0m\n' "$7" "$current_commit"
"#,
            "sh",
            &git_ref,
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

fn start_compilation(sbx: &Sandbox) -> Result<()> {
    let kernel_config = KernelConfig::default().to_string();
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
if ! git config --global --get-all safe.directory | grep -Fxq "$kernel_dir"; then
    git config --global --add safe.directory "$kernel_dir"
fi
cd "$kernel_dir"
# Write the kernel config to .config
printf '%s\n' "$1" > .config
# Build the kernel
make -j"$(nproc)" olddefconfig
make -j"$(nproc)"
"#,
            "sh",
            &kernel_config,
        ])
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
    }
}
