//! cydonia cli
#![cfg(feature = "cli")]

pub use self::{build::Build, init::Init, serve::Serve, watch::Watch};
use ccli::{clap, clap::Parser, App};

mod build;
mod init;
mod serve;
mod watch;

/// Cydonia sub command.
#[derive(Debug, Parser)]
pub enum Command {
    /// Builds a cydonia site from its markdown files
    Build(Build),
    /// Creates the boilerplate structure and files for a cydonia site
    Init(Init),
    /// Serves a cydonia site, and rebuilds it on changes
    Serve(Serve),
    /// Watches a cydonia site's files and rebuilds it on changes
    Watch(Watch),
}

/// Cydonia command line interface
#[derive(Debug, Parser)]
#[clap(author, version)]
pub struct Cydonia {
    /// The verbosity level.
    #[clap(short, long, action = clap::ArgAction::Count)]
    #[arg(global = true)]
    pub verbose: u8,

    /// The sub command.
    #[clap(subcommand)]
    pub command: Command,
}

impl App for Cydonia {
    fn verbose(&self) -> u8 {
        self.verbose
    }

    fn run(&self) -> anyhow::Result<()> {
        match &self.command {
            Command::Build(build) => build.run(),
            Command::Init(init) => init.run(),
            Command::Serve(serve) => serve.run(),
            Command::Watch(watch) => watch.run(),
        }
    }
}
