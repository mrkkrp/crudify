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
    let config_paths: Vec<_> = std::env::args_os().skip(1).collect();
    if config_paths.is_empty() {
        bail!("usage: crudify <config.yaml> [config.yaml ...]");
    }
    for config_path in config_paths {
        crudify::run(config_path)?;
    }
    Ok(())
}
