use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::model::{PressureLevel, ThermalLevel};

const SIZE_FORMAT: &str =
    "expected a byte count or a binary size such as 150G, 150GiB, 500M, or 2T";

// Binary multipliers only: `GB` is left out because it conventionally means 10^9.
const SIZE_SUFFIXES: [(&str, u64); 9] = [
    ("", 1),
    ("K", 1 << 10),
    ("KiB", 1 << 10),
    ("M", 1 << 20),
    ("MiB", 1 << 20),
    ("G", 1 << 30),
    ("GiB", 1 << 30),
    ("T", 1 << 40),
    ("TiB", 1 << 40),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub(crate) enum Format {
    Text,
    Json,
}

#[derive(Debug, Parser)]
#[command(version, about, long_about = None)]
pub(crate) struct Cli {
    /// Report format.
    #[arg(long, value_enum, default_value = "text")]
    pub(crate) format: Format,
    /// Refuse when the CPU busy ratio exceeds this value (0 to 1).
    #[arg(long, default_value = "0.90", value_parser = parse_cpu_limit)]
    pub(crate) max_cpu_busy: f64,
    /// Fall back to this one-minute load limit when CPU ticks are unavailable.
    #[arg(long, default_value = "1.0", value_parser = parse_load_limit)]
    pub(crate) max_load_per_core: f64,
    /// Refuse at or above this thermal pressure level.
    #[arg(long, value_enum, default_value = "heavy")]
    pub(crate) max_thermal: ThermalLevel,
    /// Refuse at or above this readable memory pressure level.
    #[arg(long, value_enum, default_value = "warn")]
    pub(crate) max_pressure: PressureLevel,
    /// Permit an increase in swapouts during the sampling interval.
    #[arg(long)]
    pub(crate) allow_swapping: bool,
    /// Refuse when free disk space is below this size; 0 disables the rule.
    #[arg(long, value_name = "SIZE", default_value = "150GiB", value_parser = parse_size)]
    pub(crate) min_free_disk: u64,
    /// Measure the volume that holds this path [default: home directory].
    #[arg(long, value_name = "PATH")]
    pub(crate) disk_path: Option<PathBuf>,
}

impl Cli {
    pub(crate) const fn limits(&self) -> crate::rule::Limits {
        crate::rule::Limits {
            max_cpu_busy: self.max_cpu_busy,
            max_load_per_core: self.max_load_per_core,
            max_thermal: self.max_thermal,
            max_pressure: self.max_pressure,
            allow_swapping: self.allow_swapping,
            min_free_disk_bytes: self.min_free_disk,
        }
    }
}

fn parse_cpu_limit(value: &str) -> Result<f64, &'static str> {
    let parsed = value.parse::<f64>().map_err(|_| "expected a number")?;
    if (0.0..=1.0).contains(&parsed) {
        Ok(parsed)
    } else {
        Err("expected a number from 0 to 1")
    }
}

fn parse_load_limit(value: &str) -> Result<f64, &'static str> {
    let parsed = value.parse::<f64>().map_err(|_| "expected a number")?;
    if parsed.is_finite() && parsed >= 0.0 {
        Ok(parsed)
    } else {
        Err("expected a finite, non-negative number")
    }
}

fn parse_size(value: &str) -> Result<u64, &'static str> {
    // Split off the digits by hand: `str::parse::<u64>` alone would accept a leading `+`.
    let suffix = value.trim_start_matches(|character: char| character.is_ascii_digit());
    let digits = value.strip_suffix(suffix).ok_or(SIZE_FORMAT)?;
    if digits.is_empty() {
        return Err(SIZE_FORMAT);
    }
    let multiplier = SIZE_SUFFIXES
        .iter()
        .find(|(name, _)| *name == suffix)
        .map(|(_, multiplier)| *multiplier)
        .ok_or(SIZE_FORMAT)?;
    digits
        .parse::<u64>()
        .ok()
        .and_then(|count| count.checked_mul(multiplier))
        .ok_or("size does not fit in a 64-bit byte count")
}
