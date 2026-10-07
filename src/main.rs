#[cfg(not(target_os = "macos"))]
compile_error!("headroom supports macOS only");

mod app;
mod cli;
mod model;
mod render;
mod rule;
mod sample;
#[cfg(test)]
mod tests;

use std::process::ExitCode;

fn main() -> ExitCode {
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_target(false)
        .with_writer(std::io::stderr)
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        match crate::app::run().map_err(anyhow::Error::from) {
            Ok(code) => code,
            Err(error) => {
                tracing::error!("{error:#}");
                ExitCode::from(2)
            }
        }
    })
}
