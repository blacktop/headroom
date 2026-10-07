use clap::{Parser, ValueEnum};

use crate::model::{PressureLevel, ThermalLevel};

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
    /// Refuse when the one-minute load per core exceeds this value.
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
}

impl Cli {
    pub(crate) const fn limits(&self) -> crate::rule::Limits {
        crate::rule::Limits {
            max_load_per_core: self.max_load_per_core,
            max_thermal: self.max_thermal,
            max_pressure: self.max_pressure,
            allow_swapping: self.allow_swapping,
        }
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
