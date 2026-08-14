use anyhow::Result;

use crate::{
    cli::Command,
    commands::{build::build_kernel, list::list_versions},
    tui::run_tui,
};

pub mod build;
pub mod list;

pub fn dispatch(cmd: Option<Command>) -> Result<()> {
    match cmd {
        Some(Command::Ls(args)) => list_versions(args),
        Some(Command::Build(args)) => build_kernel(args)?,
        Some(Command::Tui) | None => run_tui()?,
    }
    Ok(())
}
