use std::process::ExitCode;

use anyhow::{Result, bail};

fn main() -> ExitCode {
    match try_main() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn try_main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let Some(config_path) = args.next() else {
        bail!("usage: crudify <config.yaml>");
    };
    if args.next().is_some() {
        bail!("usage: crudify <config.yaml> (expected exactly one argument)");
    }
    crudify::run(config_path)
}
