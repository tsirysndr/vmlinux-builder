use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};

use crate::consts::KERNEL_REPO;

fn default_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|count| count.get() as u32)
        .unwrap_or(2)
}

const HELP_BANNER: &str = concat!(
    "\x1b[38;2;255;95;135m",
    " _    ____        _ _     _      \n",
    "| | _| __ ) _   _(_) | __| |_  __\n",
    "| |/ /  _ \\| | | | |/ _` \\ \\/ /\n",
    "|   <| |_) | |_| | | | (_| |>  < \n",
    "|_|\\_\\____/ \\__,_|_|_|\\__,_/_/\\_\\",
    "\x1b[0m\n",
    "\x1b[38;2;0;215;215m",
    "      Linux and BSD kernel builder",
    "\x1b[0m"
);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum BuildOs {
    #[default]
    Linux,
    Freebsd,
    Netbsd,
}

#[derive(Parser, Serialize, Deserialize)]
pub struct LsArgs {
    #[arg(
        short = 'r',
        long = "refresh",
        default_value_t = false,
        help = "Refresh the list of kernel versions by fetching from the remote repository."
    )]
    pub refresh: bool,
}

#[derive(Parser, Serialize, Deserialize)]
pub struct BuildArgs {
    #[arg(value_name = "VERSION", help = "Specify the kernel version to build.")]
    pub kernel_version: Option<String>,
    #[arg(long, value_enum, default_value_t = BuildOs::Linux, help = "Operating system kernel to build.")]
    pub os: BuildOs,
    #[arg(
        long,
        help = "Build and export a complete bootable BSD rootfs bundle with bsdkrun-agent."
    )]
    pub bundle: bool,
    #[arg(
        short = 'r',
        long = "repo",
        default_value_t = KERNEL_REPO.to_string(),
        help = "Specify a custom kernel repository URL."
    )]
    pub repo: String,
    #[arg(
        long,
        visible_alias = "ref",
        help = "Branch or tag to build from a custom repository."
    )]
    pub branch: Option<String>,
    #[arg(
        long = "version",
        value_name = "LABEL",
        help = "Version label used in artifact filenames for custom builds."
    )]
    pub version_label: Option<String>,
    #[arg(
        long,
        visible_alias = "config",
        value_name = "FILE_OR_URL",
        help = "Merge a kernel config after the built-in defaults."
    )]
    pub merge_config: Option<String>,
    #[arg(long, help = "Use a board defconfig as the base configuration.")]
    pub defconfig: Option<String>,
    #[arg(long, help = "Generate an initrd and arm64 uInitrd.")]
    pub initrd: bool,
    #[arg(long, help = "Build and archive loadable modules.")]
    pub modules: bool,
    #[arg(long, help = "Generate a U-Boot uImage on arm64.")]
    pub uimage: bool,
    #[arg(long, default_value = "arm", help = "mkimage architecture.")]
    pub uimage_arch: String,
    #[arg(long, default_value = "linux", help = "mkimage operating system.")]
    pub uimage_os: String,
    #[arg(long, default_value = "kernel", help = "mkimage image type.")]
    pub uimage_type: String,
    #[arg(long, default_value = "none", help = "mkimage compression type.")]
    pub uimage_comp: String,
    #[arg(long, default_value = "0x41000000", help = "uImage load address.")]
    pub uimage_load: String,
    #[arg(long, default_value = "0x41000000", help = "uImage entry point.")]
    pub uimage_entry: String,
    #[arg(long, help = "Override the uImage display name.")]
    pub uimage_name: Option<String>,
    #[arg(
        long,
        default_value_t = default_cpus(),
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Number of virtual CPUs assigned to the build sandbox."
    )]
    pub cpus: u32,
    #[arg(
        long,
        visible_alias = "mem",
        value_name = "MIB",
        default_value_t = 2048,
        value_parser = clap::value_parser!(u32).range(1..),
        help = "Memory assigned to the build sandbox, in MiB."
    )]
    pub memory: u32,
    #[arg(
        long = "set-config",
        value_name = "NAME=VALUE",
        help = "Override a built-in kernel config option (repeatable)."
    )]
    pub set_config: Vec<String>,
    #[arg(
        long,
        help = "Run build steps directly on a supported Linux host instead of bsdkrun."
    )]
    pub host: bool,
}

#[derive(Parser, Serialize, Deserialize)]
#[command(
    name = "kbuildx",
    version,
    before_help = HELP_BANNER,
    about = "A tool for building custom Linux, FreeBSD, and NetBSD kernels."
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Option<Command>,
}

#[derive(Subcommand, Serialize, Deserialize)]
pub enum Command {
    Ls(LsArgs),
    Build(BuildArgs),
    #[command(about = "Launch the interactive terminal interface.")]
    Tui,
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn build_resources_have_defaults() {
        let cli = Cli::try_parse_from(["kbuildx", "build", "6.6.1"]).unwrap();
        let Some(Command::Build(args)) = cli.cmd else {
            panic!("expected build command");
        };

        assert_eq!(args.cpus, super::default_cpus());
        assert_eq!(args.memory, 2048);
    }

    #[test]
    fn build_resources_can_be_overridden() {
        let cli = Cli::try_parse_from([
            "kbuildx", "build", "6.6.1", "--cpus", "4", "--memory", "4096",
        ])
        .unwrap();
        let Some(Command::Build(args)) = cli.cmd else {
            panic!("expected build command");
        };

        assert_eq!(args.cpus, 4);
        assert_eq!(args.memory, 4096);
    }

    #[test]
    fn host_build_mode_can_be_selected() {
        let cli = Cli::try_parse_from(["kbuildx", "build", "7.1.8", "--host"]).unwrap();
        let Some(Command::Build(args)) = cli.cmd else {
            panic!("expected build command");
        };

        assert!(args.host);
    }

    #[test]
    fn bsd_bundle_target_can_be_selected() {
        let cli = Cli::try_parse_from(["kbuildx", "build", "15.1", "--os", "freebsd", "--bundle"])
            .unwrap();
        let Some(Command::Build(args)) = cli.cmd else {
            panic!("expected build command");
        };

        assert_eq!(args.os, super::BuildOs::Freebsd);
        assert!(args.bundle);
    }
}
