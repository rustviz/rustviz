use clap::{Parser, Subcommand};


const DESCRIPTION: &str = concat!(crate_description!(), "\n", env!("CARGO_PKG_HOMEPAGE"));


#[derive(Parser)]
#[command(author, version, about=DESCRIPTION, long_about = None)]
pub struct Opts {
	#[command(subcommand)]
	pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
	/// Check whether a renderer is supported by this preprocessor
	Supports { renderer: String },
}


/// Parse argv and exit early on `--help` / `--version` /
/// usage errors with clap's conventional exit codes (0 for help
/// and version; 2 for argument errors). The previous version used
/// `try_parse` and routed everything through `main`'s `?`, which
/// turned `--help` into a non-zero exit with the help text printed
/// as an error blob — surprising for both interactive use and CI
/// smoke checks.
pub fn init() -> Opts {
	Opts::parse()
}
