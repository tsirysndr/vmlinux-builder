use anyhow::Result;

use crate::{
    cli::Command,
    commands::{build::build_kernel, list::list_versions},
};

pub mod build;
pub mod list;

pub fn dispatch(cmd: Command) -> Result<()> {
    match cmd {
        Command::Ls(args) => list_versions(args),
        Command::Build(args) => build_kernel(args),
    }
    Ok(())
}
