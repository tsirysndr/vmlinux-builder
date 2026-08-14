use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

use crate::consts::KERNEL_REPO;

const HELP_BANNER: &str = concat!(
    "\x1b[38;2;255;95;135m",
    " _    ____        _ _     _      \n",
    "| | _| __ ) _   _(_) | __| |_  __\n",
    "| |/ /  _ \\| | | | |/ _` \\ \\/ /\n",
    "|   <| |_) | |_| | | | (_| |>  < \n",
    "|_|\\_\\____/ \\__,_|_|_|\\__,_/_/\\_\\",
    "\x1b[0m\n",
    "\x1b[38;2;0;215;215m",
    "      Linux kernel builder",
    "\x1b[0m"
);

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
    pub version: Option<String>,
    #[arg(
        short = 'r',
        long = "repo",
        default_value_t = KERNEL_REPO.to_string(),
        help = "Specify a custom kernel repository URL."
    )]
    pub repo: String,
    #[arg(
        long,
        default_value_t = 2,
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
}

#[derive(Parser, Serialize, Deserialize)]
#[command(
    name = "kbuildx",
    version,
    before_help = HELP_BANNER,
    about = "A tool for building custom Linux kernels."
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand, Serialize, Deserialize)]
pub enum Command {
    Ls(LsArgs),
    Build(BuildArgs),
}

#[cfg(test)]
mod tests {
    use super::{Cli, Command};
    use clap::Parser;

    #[test]
    fn build_resources_have_defaults() {
        let cli = Cli::try_parse_from(["kbuildx", "build", "6.6.1"]).unwrap();
        let Command::Build(args) = cli.cmd else {
            panic!("expected build command");
        };

        assert_eq!(args.cpus, 2);
        assert_eq!(args.memory, 2048);
    }

    #[test]
    fn build_resources_can_be_overridden() {
        let cli = Cli::try_parse_from([
            "kbuildx", "build", "6.6.1", "--cpus", "4", "--memory", "4096",
        ])
        .unwrap();
        let Command::Build(args) = cli.cmd else {
            panic!("expected build command");
        };

        assert_eq!(args.cpus, 4);
        assert_eq!(args.memory, 4096);
    }
}
