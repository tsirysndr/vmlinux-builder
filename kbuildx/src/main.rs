use crate::cli::Cli;
use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;

fn main() -> Result<()> {
    commands::dispatch(Cli::parse().cmd)
}
