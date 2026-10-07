use crate::model::{PressureLevel, ReadError, Report, Sample, ThermalLevel, Verdict, VmCounters};

#[derive(Clone, Copy, Debug)]
pub(crate) struct Limits {
    pub(crate) max_load_per_core: f64,
    pub(crate) max_thermal: ThermalLevel,
    pub(crate) max_pressure: PressureLevel,
    pub(crate) allow_swapping: bool,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_load_per_core: 1.0,
            max_thermal: ThermalLevel::Heavy,
            max_pressure: PressureLevel::Warn,
            allow_swapping: false,
        }
    }
}

struct Evidence {
    load: Option<[f64; 3]>,
    ncpu: Option<u32>,
    mem_total_bytes: Option<u64>,
    vm: Option<VmCounters>,
    memory: Option<(f64, u64)>,
    thermal: Option<ThermalLevel>,
}

fn required<T: Copy>(
    value: &Result<T, ReadError>,
    sample: &str,
    name: &str,
    unavailable: &mut Vec<String>,
) -> Option<T> {
    match value {
        Ok(value) => Some(*value),
        Err(error) => {
            unavailable.push(format!("{sample} {name} unavailable: {error}"));
            None
        }
    }
}

impl Evidence {
    fn read(sample: &Sample, phase: &str, unavailable: &mut Vec<String>) -> Self {
        let load = sample.load.as_ref().ok().copied();
        let load = if load.is_some_and(|values| {
            values
                .into_iter()
                .any(|value| !value.is_finite() || value < 0.0)
        }) {
            unavailable.push(format!("{phase} load unavailable: invalid load averages"));
            None
        } else {
            required(&sample.load, phase, "load", unavailable)
        };
        let ncpu = required(&sample.ncpu, phase, "ncpu", unavailable).filter(|value| {
            if *value == 0 {
                unavailable.push(format!("{phase} ncpu unavailable: zero core count"));
            }
            *value != 0
        });
        let mem_total_bytes = required(&sample.mem_total_bytes, phase, "memory size", unavailable)
            .filter(|value| {
                if *value == 0 {
                    unavailable.push(format!("{phase} memory size unavailable: zero bytes"));
                }
                *value != 0
            });
        let vm = required(&sample.vm, phase, "VM counters", unavailable);
        let memory =
            vm.zip(mem_total_bytes)
                .and_then(|(vm, total)| match memory_metrics(vm, total) {
                    Ok(metrics) => Some(metrics),
                    Err(error) => {
                        unavailable.push(format!("{phase} VM counters unavailable: {error}"));
                        None
                    }
                });
        let thermal = required(&sample.thermal, phase, "thermal state", unavailable);
        Self {
            load,
            ncpu,
            mem_total_bytes,
            vm,
            memory,
            thermal,
        }
    }
}

fn memory_metrics(vm: VmCounters, total: u64) -> Result<(f64, u64), &'static str> {
    if !vm.page_size.is_power_of_two() {
        return Err("invalid kernel page size");
    }
    let free_bytes = vm
        .free_count
        .checked_add(vm.inactive_count)
        .and_then(|value| value.checked_add(vm.speculative_count))
        .and_then(|value| value.checked_add(vm.purgeable_count))
        .and_then(|value| value.checked_mul(vm.page_size))
        .ok_or("free memory byte count overflow")?;
    let compressor_bytes = vm
        .compressor_page_count
        .checked_mul(vm.page_size)
        .ok_or("compressor byte count overflow")?;
    // The requested estimate overlaps some VM categories, so cap it at total RAM.
    Ok((fraction(free_bytes.min(total), total), compressor_bytes))
}

#[expect(
    clippy::cast_precision_loss,
    reason = "bounded display ratio; byte counts stay integers"
)]
fn fraction(numerator: u64, denominator: u64) -> f64 {
    numerator as f64 / denominator as f64
}

fn swap_delta(before: &Evidence, after: &Evidence, unavailable: &mut Vec<String>) -> Option<u64> {
    before.vm.zip(after.vm).and_then(|(before, after)| {
        let delta = after
            .swapouts
            .checked_sub(before.swapouts)
            .filter(|_| after.swapins >= before.swapins);
        if delta.is_none() {
            unavailable.push("VM counters unavailable: swap counters decreased".to_owned());
        }
        delta
    })
}

pub(crate) fn evaluate(before: &Sample, after: &Sample, limits: &Limits) -> Report {
    let mut reasons = Vec::new();
    let first = Evidence::read(before, "first", &mut reasons);
    let latest = Evidence::read(after, "second", &mut reasons);
    let delta = swap_delta(&first, &latest, &mut reasons);
    let load1_per_core = latest
        .load
        .zip(latest.ncpu)
        .map(|([load1, _, _], ncpu)| load1 / f64::from(ncpu));
    let incomplete = !reasons.is_empty();
    record_refusals(
        &latest,
        after.pressure,
        delta,
        load1_per_core,
        limits,
        &mut reasons,
    );
    let verdict = if incomplete {
        Verdict::Unknown
    } else if reasons.is_empty() {
        Verdict::Admit
    } else {
        Verdict::Refuse
    };
    Report {
        schema: 1,
        verdict,
        reasons,
        load1_per_core,
        load1: latest.load.map(|[value, _, _]| value),
        load5: latest.load.map(|[_, value, _]| value),
        load15: latest.load.map(|[_, _, value]| value),
        ncpu: latest.ncpu,
        mem_total_bytes: latest.mem_total_bytes,
        mem_free_ratio: latest.memory.map(|(ratio, _)| ratio),
        compressor_bytes: latest.memory.map(|(_, bytes)| bytes),
        swapouts_delta: delta,
        swap_used_bytes: after.swap_used_bytes,
        pressure_level: after.pressure,
        thermal_level: latest.thermal,
    }
}

fn record_refusals(
    latest: &Evidence,
    pressure: Option<PressureLevel>,
    delta: Option<u64>,
    load_per_core: Option<f64>,
    limits: &Limits,
    reasons: &mut Vec<String>,
) {
    // Stable severity order: thermal emergency, memory pressure, active swap, load.
    if let Some(level) = latest.thermal.filter(|level| *level >= limits.max_thermal) {
        reasons.push(format!("thermal {level} >= {}", limits.max_thermal));
    }
    if let Some(level) = pressure.filter(|level| *level >= limits.max_pressure) {
        reasons.push(format!(
            "memory pressure {level} >= {}",
            limits.max_pressure
        ));
    }
    if let Some(pages) = delta.filter(|pages| *pages > 0 && !limits.allow_swapping) {
        reasons.push(format!("swapouts increased by {pages}"));
    }
    if let Some(load) = load_per_core.filter(|value| *value > limits.max_load_per_core) {
        reasons.push(format!(
            "load per core {load:.2} > {}",
            limits.max_load_per_core
        ));
    }
}
