use owo_colors::OwoColorize;

use crate::{cli::BuildArgs, config::KernelConfig};

pub fn build_kernel(args: BuildArgs) {
    let repo = args.repo.as_str();
    let version = args.version.unwrap_or_else(|| fetch_last_version(repo));
    println!(
        "=> Build {} {}",
        OwoColorize::bright_purple(&repo),
        OwoColorize::dimmed(&version)
    );
    println!("{}", KernelConfig::default().to_string());
}

fn fetch_last_version(repo: &str) -> String {
    let output = std::process::Command::new("git")
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
