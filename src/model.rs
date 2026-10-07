use std::fmt;
use std::io;

use clap::ValueEnum;
use serde::Serialize;
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThermalLevel {
    Nominal,
    Moderate,
    Heavy,
    Trapping,
    Sleeping,
}

impl ThermalLevel {
    // macOS branch of SDK libkern/OSThermalNotification.h; iOS uses tens.
    pub(crate) fn from_raw(value: u64) -> Result<Self, ReadError> {
        match value {
            0 => Ok(Self::Nominal),
            1 => Ok(Self::Moderate),
            2 => Ok(Self::Heavy),
            3 => Ok(Self::Trapping),
            4 => Ok(Self::Sleeping),
            _ => Err(ReadError::UnexpectedThermal(value)),
        }
    }
}

impl fmt::Display for ThermalLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Nominal => "nominal",
            Self::Moderate => "moderate",
            Self::Heavy => "heavy",
            Self::Trapping => "trapping",
            Self::Sleeping => "sleeping",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PressureLevel {
    Normal,
    Warn,
    Critical,
}

impl PressureLevel {
    pub(crate) fn from_raw(value: i32) -> Result<Self, ReadError> {
        match value {
            1 => Ok(Self::Normal),
            2 => Ok(Self::Warn),
            4 => Ok(Self::Critical),
            _ => Err(ReadError::Invalid("unrecognized memory pressure level")),
        }
    }
}

impl fmt::Display for PressureLevel {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Normal => "normal",
            Self::Warn => "warn",
            Self::Critical => "critical",
        })
    }
}

#[derive(Debug, Error)]
pub(crate) enum ReadError {
    #[error("unrecognized thermal pressure level {0} (expected macOS values 0 through 4)")]
    UnexpectedThermal(u64),
    #[error("{operation}: {source}")]
    Os {
        operation: &'static str,
        source: io::Error,
    },
    #[error("{operation}: status {code}")]
    Status { operation: &'static str, code: i64 },
    #[error("{0}")]
    Invalid(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct VmCounters {
    pub(crate) free_count: u64,
    pub(crate) inactive_count: u64,
    pub(crate) speculative_count: u64,
    pub(crate) purgeable_count: u64,
    pub(crate) compressor_page_count: u64,
    pub(crate) swapins: u64,
    pub(crate) swapouts: u64,
    pub(crate) page_size: u64,
}

#[derive(Debug)]
pub(crate) struct Sample {
    pub(crate) load: Result<[f64; 3], ReadError>,
    pub(crate) ncpu: Result<u32, ReadError>,
    pub(crate) mem_total_bytes: Result<u64, ReadError>,
    pub(crate) vm: Result<VmCounters, ReadError>,
    pub(crate) thermal: Result<ThermalLevel, ReadError>,
    pub(crate) swap_used_bytes: Option<u64>,
    pub(crate) pressure: Option<PressureLevel>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub(crate) enum Verdict {
    Admit,
    Refuse,
    Unknown,
}

impl Verdict {
    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::Admit => 0,
            Self::Refuse => 1,
            Self::Unknown => 2,
        }
    }
}

impl fmt::Display for Verdict {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Admit => "ADMIT",
            Self::Refuse => "REFUSE",
            Self::Unknown => "UNKNOWN",
        })
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Report {
    pub(crate) schema: u8,
    pub(crate) verdict: Verdict,
    pub(crate) reasons: Vec<String>,
    pub(crate) load1_per_core: Option<f64>,
    pub(crate) load1: Option<f64>,
    pub(crate) load5: Option<f64>,
    pub(crate) load15: Option<f64>,
    pub(crate) ncpu: Option<u32>,
    pub(crate) mem_total_bytes: Option<u64>,
    pub(crate) mem_free_ratio: Option<f64>,
    pub(crate) compressor_bytes: Option<u64>,
    pub(crate) swapouts_delta: Option<u64>,
    pub(crate) swap_used_bytes: Option<u64>,
    pub(crate) pressure_level: Option<PressureLevel>,
    pub(crate) thermal_level: Option<ThermalLevel>,
}
