//! Hub package management command.

use crate::repl::runner::Runner;
use anyhow::Result;
use clap::{Args, Subcommand};
use futures_util::StreamExt;
use wcore::protocol::message::{HubAction, HubEvent};

/// Manage hub packages.
#[derive(Args, Debug)]
pub struct Hub {
    /// Hub subcommand.
    #[command(subcommand)]
    pub command: HubCommand,
}

/// Hub subcommands.
#[derive(Subcommand, Debug)]
pub enum HubCommand {
    /// Install a hub package.
    Install(HubPackage),
    /// Uninstall a hub package.
    Uninstall(HubPackage),
}

/// Package argument shared by install and uninstall.
#[derive(Args, Debug)]
pub struct HubPackage {
    /// Package identifier in `scope/name` format.
    pub package: String,
}

impl Hub {
    /// Run the hub command.
    pub async fn run(self, runner: &mut Runner) -> Result<()> {
        let (package, action) = match self.command {
            HubCommand::Install(p) => (p.package, HubAction::Install),
            HubCommand::Uninstall(p) => (p.package, HubAction::Uninstall),
        };
        let stream = runner.hub_stream(&package, action);
        futures_util::pin_mut!(stream);
        while let Some(result) = stream.next().await {
            match result? {
                HubEvent::Start { package } => println!("Starting hub operation for {package}..."),
                HubEvent::Step { message } => println!("  {message}"),
                HubEvent::End { package } => println!("Done: {package}"),
            }
        }
        Ok(())
    }
}
