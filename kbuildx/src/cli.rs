use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};

#[derive(Parser, Serialize, Deserialize)]
pub struct LsArgs {
    #[arg(long)]
    pub refresh: bool,
}

#[derive(Parser, Serialize, Deserialize)]
#[command(
    name = "kbuildx",
    version,
    about = "A tool for building custom Linux kernels."
)]
pub struct Cli {
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand, Serialize, Deserialize)]
pub enum Command {
    Ls(LsArgs),
}
