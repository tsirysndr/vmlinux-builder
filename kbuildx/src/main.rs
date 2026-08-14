use crate::cli::Cli;
use anyhow::Result;
use clap::Parser;

mod cli;
mod commands;
mod config;
mod consts;
mod tui;

fn main() -> Result<()> {
    commands::dispatch(Cli::parse().cmd)
}
