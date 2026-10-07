#![expect(
    clippy::panic_in_result_fn,
    reason = "test assertions should fail the test while setup errors use ?"
)]

use std::error::Error;
use std::io;

use clap::Parser;
use serde_json::json;

use crate::cli::{Cli, Format};
use crate::model::{PressureLevel, ReadError, Sample, ThermalLevel, Verdict, VmCounters};
use crate::rule::{Limits, evaluate};

type TestResult = Result<(), Box<dyn Error>>;

fn vm(sample: &mut Sample) -> Result<&mut VmCounters, ReadError> {
    sample
        .vm
        .as_mut()
        .map_err(|_| ReadError::Invalid("invalid test fixture"))
}

fn sample() -> Sample {
    Sample {
        load: Ok([4.0, 6.0, 12.0]),
        ncpu: Ok(16),
        mem_total_bytes: Ok(16_384_000),
        vm: Ok(VmCounters {
            free_count: 200,
            inactive_count: 200,
            speculative_count: 50,
            purgeable_count: 50,
            compressor_page_count: 100,
            swapins: 11,
            swapouts: 22,
            page_size: 16_384,
        }),
        thermal: Ok(ThermalLevel::Nominal),
        swap_used_bytes: None,
        pressure: None,
    }
}

fn denied() -> ReadError {
    ReadError::Os {
        operation: "injected read",
        source: io::Error::from_raw_os_error(libc::EPERM),
    }
}

#[test]
fn load_threshold_is_strict_and_configurable() {
    let before = sample();
    let mut after = sample();
    after.load = Ok([16.0, 20.0, 30.0]);
    assert_eq!(
        evaluate(&before, &after, &Limits::default()).verdict,
        Verdict::Admit
    );
    after.load = Ok([16.1, 0.0, 0.0]);
    let report = evaluate(&before, &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Refuse);
    assert_eq!(report.reasons, ["load per core 1.01 > 1"]);
    let limits = Limits {
        max_load_per_core: 2.0,
        ..Limits::default()
    };
    assert_eq!(evaluate(&before, &after, &limits).verdict, Verdict::Admit);
}

#[test]
fn thermal_threshold_is_inclusive_and_configurable() {
    for (level, expected) in [
        (ThermalLevel::Nominal, Verdict::Admit),
        (ThermalLevel::Moderate, Verdict::Admit),
        (ThermalLevel::Heavy, Verdict::Refuse),
        (ThermalLevel::Trapping, Verdict::Refuse),
        (ThermalLevel::Sleeping, Verdict::Refuse),
    ] {
        let mut after = sample();
        after.thermal = Ok(level);
        assert_eq!(
            evaluate(&sample(), &after, &Limits::default()).verdict,
            expected
        );
    }
    let mut after = sample();
    after.thermal = Ok(ThermalLevel::Heavy);
    let limits = Limits {
        max_thermal: ThermalLevel::Trapping,
        ..Limits::default()
    };
    assert_eq!(evaluate(&sample(), &after, &limits).verdict, Verdict::Admit);
}

#[test]
fn pressure_threshold_is_inclusive_and_configurable() {
    for (level, expected) in [
        (PressureLevel::Normal, Verdict::Admit),
        (PressureLevel::Warn, Verdict::Refuse),
        (PressureLevel::Critical, Verdict::Refuse),
    ] {
        let mut after = sample();
        after.pressure = Some(level);
        assert_eq!(
            evaluate(&sample(), &after, &Limits::default()).verdict,
            expected
        );
    }
    let mut after = sample();
    after.pressure = Some(PressureLevel::Warn);
    let limits = Limits {
        max_pressure: PressureLevel::Critical,
        ..Limits::default()
    };
    assert_eq!(evaluate(&sample(), &after, &limits).verdict, Verdict::Admit);
}

#[test]
fn swapping_can_be_allowed_without_hiding_the_delta() -> TestResult {
    let mut after = sample();
    vm(&mut after)?.swapouts = 23;
    let report = evaluate(&sample(), &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Refuse);
    assert_eq!(report.reasons, ["swapouts increased by 1"]);
    let limits = Limits {
        allow_swapping: true,
        ..Limits::default()
    };
    let report = evaluate(&sample(), &after, &limits);
    assert_eq!(report.verdict, Verdict::Admit);
    assert_eq!(report.swapouts_delta, Some(1));
    Ok(())
}

#[test]
fn every_triggered_reason_is_reported_in_severity_order() -> TestResult {
    let mut after = sample();
    after.thermal = Ok(ThermalLevel::Sleeping);
    after.pressure = Some(PressureLevel::Critical);
    vm(&mut after)?.swapouts = 25;
    after.load = Ok([32.0, 0.0, 0.0]);
    let report = evaluate(&sample(), &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Refuse);
    assert_eq!(
        report.reasons,
        [
            "thermal sleeping >= heavy",
            "memory pressure critical >= warn",
            "swapouts increased by 3",
            "load per core 2.00 > 1",
        ]
    );
    Ok(())
}

#[test]
fn either_required_sample_being_unavailable_is_unknown() {
    for first_missing in [true, false] {
        for field in 0..5 {
            let mut before = sample();
            let mut after = sample();
            let target = if first_missing {
                &mut before
            } else {
                &mut after
            };
            match field {
                0 => target.load = Err(denied()),
                1 => target.ncpu = Err(denied()),
                2 => target.mem_total_bytes = Err(denied()),
                3 => target.vm = Err(denied()),
                _ => target.thermal = Err(denied()),
            }
            let report = evaluate(&before, &after, &Limits::default());
            assert_eq!(
                report.verdict,
                Verdict::Unknown,
                "field {field}, first={first_missing}"
            );
            assert!(
                report
                    .reasons
                    .iter()
                    .any(|reason| reason.contains("Operation not permitted"))
            );
        }
    }
}

#[test]
fn unavailable_required_evidence_takes_precedence_over_known_refusals() {
    let mut after = sample();
    after.load = Err(denied());
    after.thermal = Ok(ThermalLevel::Heavy);
    let report = evaluate(&sample(), &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Unknown);
    assert!(
        report
            .reasons
            .iter()
            .any(|reason| reason == "thermal heavy >= heavy")
    );
}

#[test]
fn malformed_external_numbers_are_unknown() -> TestResult {
    for case in 0..11 {
        let mut after = sample();
        match case {
            0 => after.ncpu = Ok(0),
            1 => after.mem_total_bytes = Ok(0),
            2 => after.load = Ok([f64::NAN, 0.0, 0.0]),
            3 => after.load = Ok([0.0, f64::INFINITY, 0.0]),
            4 => after.load = Ok([0.0, 0.0, -1.0]),
            5 => vm(&mut after)?.page_size = 0,
            6 => vm(&mut after)?.free_count = u64::MAX,
            7 => vm(&mut after)?.compressor_page_count = u64::MAX,
            8 => vm(&mut after)?.swapouts = 21,
            9 => vm(&mut after)?.swapins = 10,
            _ => vm(&mut after)?.page_size = 3,
        }
        assert_eq!(
            evaluate(&sample(), &after, &Limits::default()).verdict,
            Verdict::Unknown,
            "case {case}"
        );
    }
    Ok(())
}

#[test]
fn free_memory_estimate_is_bounded_when_vm_categories_overlap() -> TestResult {
    let mut after = sample();
    vm(&mut after)?.free_count = 1_000;
    let report = evaluate(&sample(), &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Admit);
    assert_eq!(report.mem_free_ratio, Some(1.0));
    Ok(())
}

#[test]
fn sdk_thermal_levels_match_process_info_severity_semantics() -> TestResult {
    // OSThermalNotification.h has five macOS levels. NSProcessInfo.h documents
    // nominal=normal, fair=slightly elevated, serious=high, critical=must cool.
    // The two highest pressure levels both belong to the critical severity.
    // tests/sdk_contract.m verifies the named SDK constants independently.
    for (raw, level, process_info_severity) in [
        (0, ThermalLevel::Nominal, 0),
        (1, ThermalLevel::Moderate, 1),
        (2, ThermalLevel::Heavy, 2),
        (3, ThermalLevel::Trapping, 3),
        (4, ThermalLevel::Sleeping, 3),
    ] {
        assert_eq!(ThermalLevel::from_raw(raw)?, level);
        assert_eq!(level >= ThermalLevel::Heavy, process_info_severity >= 2);
    }
    assert!(ThermalLevel::from_raw(10).is_err());
    assert!(ThermalLevel::from_raw(u64::MAX).is_err());
    Ok(())
}

#[test]
fn pressure_values_are_the_kernel_levels() -> TestResult {
    assert_eq!(PressureLevel::from_raw(1)?, PressureLevel::Normal);
    assert_eq!(PressureLevel::from_raw(2)?, PressureLevel::Warn);
    assert_eq!(PressureLevel::from_raw(4)?, PressureLevel::Critical);
    assert!(PressureLevel::from_raw(3).is_err());
    Ok(())
}

#[test]
fn mach_binding_fields_match_the_sdk_contract() {
    use mach2::vm_statistics::vm_statistics64;
    use std::mem::offset_of;

    // The companion C assertions in tests/sdk_contract.m use these same
    // offsets against the installed SDK, including the minimum reply size.
    assert_eq!(offset_of!(vm_statistics64, free_count), 0);
    assert_eq!(offset_of!(vm_statistics64, inactive_count), 8);
    assert_eq!(offset_of!(vm_statistics64, purgeable_count), 88);
    assert_eq!(offset_of!(vm_statistics64, speculative_count), 92);
    assert_eq!(offset_of!(vm_statistics64, swapins), 112);
    assert_eq!(offset_of!(vm_statistics64, swapouts), 120);
    assert_eq!(offset_of!(vm_statistics64, compressor_page_count), 128);
    assert_eq!(offset_of!(vm_statistics64, swapped_count), 152);
}

#[test]
fn text_report_has_the_documented_keys_order_and_precision() -> TestResult {
    let report = evaluate(&sample(), &sample(), &Limits::default());
    let mut output = Vec::new();
    crate::render::write(&mut output, &report, Format::Text)?;
    assert_eq!(
        String::from_utf8(output)?,
        concat!(
            "ADMIT: load 4.0 on 16 cores, memory free 0.50, thermal nominal\n",
            "load1_per_core: 0.25\nload1: 4.0\nload5: 6.0\nload15: 12.0\n",
            "ncpu: 16\nmem_total_bytes: 16384000\nmem_free_ratio: 0.50\n",
            "compressor_bytes: 1638400\nswapouts_delta: 0\nswap_used_bytes: null\n",
            "pressure_level: null\nthermal_level: nominal\n",
        )
    );
    Ok(())
}

#[test]
fn json_report_is_one_flat_versioned_object_with_exact_integers() -> TestResult {
    let mut after = sample();
    after.swap_used_bytes = Some(9_007_199_254_740_993);
    after.pressure = Some(PressureLevel::Normal);
    let report = evaluate(&sample(), &after, &Limits::default());
    let mut output = Vec::new();
    crate::render::write(&mut output, &report, Format::Json)?;
    let text = String::from_utf8(output)?;
    assert_eq!(text.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&text)?,
        json!({
            "schema": 1, "verdict": "ADMIT", "reasons": [],
            "load1_per_core": 0.25, "load1": 4.0, "load5": 6.0, "load15": 12.0,
            "ncpu": 16, "mem_total_bytes": 16_384_000, "mem_free_ratio": 0.5,
            "compressor_bytes": 1_638_400, "swapouts_delta": 0,
            "swap_used_bytes": 9_007_199_254_740_993_u64,
            "pressure_level": "normal", "thermal_level": "nominal",
        })
    );
    Ok(())
}

#[test]
fn unknown_output_keeps_missing_fields_null() -> TestResult {
    let mut after = sample();
    after.load = Err(ReadError::Invalid("injected failure"));
    let report = evaluate(&sample(), &after, &Limits::default());
    let mut output = Vec::new();
    crate::render::write(&mut output, &report, Format::Text)?;
    let text = String::from_utf8(output)?;
    assert!(text.starts_with("UNKNOWN: second load unavailable: injected failure\n"));
    assert!(text.contains("load1_per_core: null\nload1: null\nload5: null\nload15: null\n"));
    let value = serde_json::to_value(report)?;
    assert_eq!(value.get("load1"), Some(&serde_json::Value::Null));
    Ok(())
}

#[test]
fn refusal_text_first_line_contains_all_reasons() -> TestResult {
    let mut after = sample();
    after.thermal = Ok(ThermalLevel::Heavy);
    after.pressure = Some(PressureLevel::Warn);
    let report = evaluate(&sample(), &after, &Limits::default());
    let mut output = Vec::new();
    crate::render::write(&mut output, &report, Format::Text)?;
    assert!(
        String::from_utf8(output)?
            .starts_with("REFUSE: thermal heavy >= heavy; memory pressure warn >= warn\n")
    );
    Ok(())
}

#[test]
fn cli_defaults_match_the_rule_defaults() -> TestResult {
    let cli = Cli::try_parse_from(["headroom"])?;
    let limits = Limits::default();
    assert_eq!(cli.format, Format::Text);
    assert!((cli.max_load_per_core - limits.max_load_per_core).abs() < f64::EPSILON);
    assert_eq!(cli.max_thermal, limits.max_thermal);
    assert_eq!(cli.max_pressure, limits.max_pressure);
    assert_eq!(cli.allow_swapping, limits.allow_swapping);
    Ok(())
}

#[test]
fn cli_rejects_unknown_flags_and_invalid_thresholds() {
    for arguments in [
        vec!["headroom", "--unknown"],
        vec!["headroom", "positional"],
        vec!["headroom", "--format", "xml"],
        vec!["headroom", "--max-load-per-core=NaN"],
        vec!["headroom", "--max-load-per-core=inf"],
        vec!["headroom", "--max-load-per-core=-1"],
        vec!["headroom", "--max-thermal", "serious"],
        vec!["headroom", "--max-pressure", "2"],
    ] {
        let result = Cli::try_parse_from(arguments);
        assert!(result.is_err());
        if let Err(error) = result {
            assert_eq!(error.exit_code(), 2);
        }
    }
}

#[test]
fn cli_accepts_every_documented_threshold_flag() -> TestResult {
    let cli = Cli::try_parse_from([
        "headroom",
        "--format",
        "json",
        "--max-load-per-core",
        "2.5",
        "--max-thermal",
        "sleeping",
        "--max-pressure",
        "critical",
        "--allow-swapping",
    ])?;
    assert_eq!(cli.format, Format::Json);
    assert_eq!(cli.max_thermal, ThermalLevel::Sleeping);
    assert_eq!(cli.max_pressure, PressureLevel::Critical);
    assert!(cli.allow_swapping);
    assert!((cli.max_load_per_core - 2.5).abs() < f64::EPSILON);
    Ok(())
}

#[test]
fn verdict_exit_codes_are_stable() {
    assert_eq!(
        [
            Verdict::Admit.code(),
            Verdict::Refuse.code(),
            Verdict::Unknown.code()
        ],
        [0, 1, 2]
    );
}
