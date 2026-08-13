use std::{path::Path, process::Command};

use crate::cli::LsArgs;

const KERNEL_REPO: &str = "https://git.kernel.org/pub/scm/linux/kernel/git/stable/linux.git";

pub fn list_versions(args: LsArgs) {
    if Path::new("/tmp/kernel_versions.txt").exists() && !args.refresh {
        let output = std::fs::read_to_string("/tmp/kernel_versions.txt")
            .expect("Failed to read /tmp/kernel_versions.txt");

        let versions: Vec<&str> = output
            .lines()
            .filter(|line| !line.ends_with("^{}"))
            .filter_map(|line| line.split("refs/tags/v").nth(1))
            .collect();

        for version in versions {
            println!("{}", version);
        }

        // if file exists and is older than 24 hours, update it
        let metadata = std::fs::metadata("/tmp/kernel_versions.txt")
            .expect("Failed to get metadata for /tmp/kernel_versions.txt");
        let modified = metadata.modified().expect("Failed to get modified time");
        let now = std::time::SystemTime::now();

        if now.duration_since(modified).unwrap().as_secs() > 24 * 60 * 60 {
            cache_versions();
        }

        return;
    }

    let output = cache_versions();
    let versions: Vec<&str> = output
        .lines()
        .filter(|line| !line.ends_with("^{}"))
        .filter_map(|line| line.split("refs/tags/v").nth(1))
        .collect();

    for version in versions {
        println!("{}", version);
    }
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
