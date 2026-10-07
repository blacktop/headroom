use std::fmt::{self, Display};
use std::io::{self, Write};

use thiserror::Error;

use crate::cli::Format;
use crate::model::{Report, Verdict};

#[derive(Debug, Error)]
pub(crate) enum RenderError {
    #[error("cannot write report: {0}")]
    Io(#[from] io::Error),
    #[error("cannot serialize report: {0}")]
    Json(#[from] serde_json::Error),
}

struct Nullable<T>(Option<T>);

impl<T: Display> Display for Nullable<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Some(value) => Display::fmt(value, formatter),
            None => formatter.write_str("null"),
        }
    }
}

pub(crate) fn write(
    output: &mut impl Write,
    report: &Report,
    format: Format,
) -> Result<(), RenderError> {
    match format {
        Format::Text => text(output, report)?,
        Format::Json => {
            serde_json::to_writer(&mut *output, report)?;
            writeln!(output)?;
        }
    }
    Ok(())
}

fn text(output: &mut impl Write, report: &Report) -> io::Result<()> {
    if report.verdict == Verdict::Admit {
        write!(
            output,
            "ADMIT: cpu busy {:.2}, load {:.1} on {} cores, memory free {:.2}, thermal {}",
            Nullable(report.cpu_busy_ratio),
            Nullable(report.load1),
            Nullable(report.ncpu),
            Nullable(report.mem_free_ratio),
            Nullable(report.thermal_level)
        )?;
    } else {
        write!(output, "{}: {}", report.verdict, report.reasons.join("; "))?;
    }
    if report.cpu_busy_ratio.is_none() {
        write!(output, "; cpu ticks unavailable, using load fallback")?;
    }
    writeln!(output)?;
    writeln!(
        output,
        "cpu_busy_ratio: {:.2}",
        Nullable(report.cpu_busy_ratio)
    )?;
    writeln!(
        output,
        "load1_per_core: {:.2}",
        Nullable(report.load1_per_core)
    )?;
    writeln!(output, "load1: {:.1}", Nullable(report.load1))?;
    writeln!(output, "load5: {:.1}", Nullable(report.load5))?;
    writeln!(output, "load15: {:.1}", Nullable(report.load15))?;
    writeln!(output, "ncpu: {}", Nullable(report.ncpu))?;
    writeln!(
        output,
        "mem_total_bytes: {}",
        Nullable(report.mem_total_bytes)
    )?;
    writeln!(
        output,
        "mem_free_ratio: {:.2}",
        Nullable(report.mem_free_ratio)
    )?;
    writeln!(
        output,
        "compressor_bytes: {}",
        Nullable(report.compressor_bytes)
    )?;
    writeln!(
        output,
        "swapouts_delta: {}",
        Nullable(report.swapouts_delta)
    )?;
    writeln!(
        output,
        "swap_used_bytes: {}",
        Nullable(report.swap_used_bytes)
    )?;
    writeln!(
        output,
        "pressure_level: {}",
        Nullable(report.pressure_level)
    )?;
    writeln!(output, "thermal_level: {}", Nullable(report.thermal_level))
}
