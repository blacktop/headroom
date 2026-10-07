use std::io::{self, Write};
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;

use crate::cli::Cli;
use crate::render::RenderError;

pub(crate) fn run() -> Result<ExitCode, RenderError> {
    let options = match Cli::try_parse() {
        Ok(options) => options,
        Err(error) => {
            if error.use_stderr() {
                tracing::error!("{error}");
                return Ok(ExitCode::from(2));
            }
            write!(io::stdout().lock(), "{error}")?;
            return Ok(ExitCode::SUCCESS);
        }
    };
    let before = crate::sample::take();
    std::thread::sleep(Duration::from_secs(1));
    let after = crate::sample::take();
    let report = crate::rule::evaluate(&before, &after, &options.limits());
    let mut output = io::stdout().lock();
    crate::render::write(&mut output, &report, options.format)?;
    output.flush()?;
    Ok(ExitCode::from(report.verdict.code()))
}
