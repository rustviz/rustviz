#[macro_use]
extern crate log;
extern crate env_logger;
#[macro_use]
extern crate clap;

use std::process;
use std::error::Error;
use mdbook_preprocessor::Preprocessor;
use std::env;
use std::path::Path;

mod cli;
mod cfg;
mod preprocessor;

pub type Result<Ok = (), Err = Box<dyn Error>> = std::result::Result<Ok, Err>;


fn main() -> Result {
	// `cli::init()` no longer returns Result — clap handles
	// --help/--version/usage internally and exits with the right
	// status code before we get here.
	let opts = cli::init();
	env_logger::init();


	// get cwd
	if let Ok(current_dir) = env::current_dir(){
		let c_path: &Path = current_dir.as_path();
		info!("cwd: {:#?}", c_path.as_os_str());

		let rv = preprocessor::RustvizPlugin::new(c_path);
		if let Some(cli::Commands::Supports { renderer }) = opts.command {
			// Signal whether the renderer is supported by exiting with 1 or 0.
			// `supports_renderer` returns `Result<bool>` since mdbook 0.5;
			// surface any error as not-supported so we don't silently exit 0
			// on a misconfigured plugin.
			match rv.supports_renderer(&renderer) {
				Ok(true) => process::exit(0),
				_ => process::exit(1),
			}
		} else if let Err(e) = rv.handle_preprocessing() {
			error!("{}", e);
			process::exit(1);
		}
		
	}

	Ok(())
}