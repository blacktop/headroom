#![expect(
    clippy::panic_in_result_fn,
    reason = "test assertions should fail the test while setup errors use ?"
)]

use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use clap::Parser;
use serde_json::json;

use crate::cli::{Cli, Format};
use crate::model::{
    BYTES_PER_GIB, PressureLevel, ReadError, Sample, ThermalLevel, Verdict, VmCounters,
};
use crate::rule::{Limits, evaluate};
use crate::sample::{available_bytes, disk_free_bytes, home_directory};

type TestResult = Result<(), Box<dyn Error>>;

fn vm(sample: &mut Sample) -> Result<&mut VmCounters, ReadError> {
    sample
        .vm
        .as_mut()
        .map_err(|_| ReadError::Invalid("invalid test fixture"))
}

fn sample() -> Sample {
    Sample {
        cpu_ticks: Ok([0; 4]),
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
        disk_free_bytes: Ok(400 * BYTES_PER_GIB),
    }
}

fn after_sample() -> Sample {
    Sample {
        cpu_ticks: Ok([30, 20, 25, 25]),
        ..sample()
    }
}

fn denied() -> ReadError {
    ReadError::Os {
        operation: "injected read",
        source: io::Error::from_raw_os_error(libc::EPERM),
    }
}

#[test]
fn cpu_threshold_is_strict_and_configurable() {
    let before = sample();
    let mut after = after_sample();
    after.load = Ok([32.0, 40.0, 60.0]);
    for (ticks, ratio, expected) in [
        ([0, 0, 100, 0], 0.0, Verdict::Admit),
        ([30, 20, 25, 25], 0.75, Verdict::Admit),
        ([70, 10, 10, 10], 0.90, Verdict::Admit),
        ([71, 10, 9, 10], 0.91, Verdict::Refuse),
        ([80, 10, 0, 10], 1.0, Verdict::Refuse),
    ] {
        after.cpu_ticks = Ok(ticks);
        let report = evaluate(&before, &after, &Limits::default());
        assert_eq!(report.verdict, expected);
        assert_eq!(report.cpu_busy_ratio, Some(ratio));
    }
    assert_eq!(
        evaluate(&before, &after, &Limits::default()).reasons,
        ["cpu busy 1.00 > 0.90"]
    );
    let limits = Limits {
        max_cpu_busy: 1.0,
        ..Limits::default()
    };
    assert_eq!(evaluate(&before, &after, &limits).verdict, Verdict::Admit);
}

#[test]
fn load_fallback_is_strict_and_configurable() {
    let before = sample();
    let mut after = after_sample();
    after.cpu_ticks = Err(denied());
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
fn either_unavailable_cpu_sample_uses_load_fallback() {
    for first_missing in [true, false] {
        let mut before = sample();
        let mut after = after_sample();
        if first_missing {
            before.cpu_ticks = Err(denied());
        } else {
            after.cpu_ticks = Err(denied());
        }
        let report = evaluate(&before, &after, &Limits::default());
        assert_eq!(report.verdict, Verdict::Admit);
        assert_eq!(report.cpu_busy_ratio, None);
        after.load = Ok([32.0, 0.0, 0.0]);
        let report = evaluate(&before, &after, &Limits::default());
        assert_eq!(report.verdict, Verdict::Refuse);
        assert_eq!(report.reasons, ["load per core 2.00 > 1"]);
    }
}

#[test]
fn zero_cpu_delta_is_unavailable_and_uses_load_fallback() {
    let before = sample();
    let mut after = sample();
    let report = evaluate(&before, &after, &Limits::default());
    assert_eq!(report.cpu_busy_ratio, None);
    assert_eq!(report.verdict, Verdict::Admit);
    after.load = Ok([32.0, 0.0, 0.0]);
    let report = evaluate(&before, &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Refuse);
    assert_eq!(report.cpu_busy_ratio, None);
}

#[test]
fn cpu_tick_wrap_and_wide_totals_preserve_the_ratio() {
    let mut before = sample();
    before.cpu_ticks = Ok([u32::MAX - 9; 4]);
    let mut after = after_sample();
    after.cpu_ticks = Ok([0; 4]);
    assert_eq!(
        evaluate(&before, &after, &Limits::default()).cpu_busy_ratio,
        Some(0.75)
    );
    before.cpu_ticks = Ok([0; 4]);
    after.cpu_ticks = Ok([u32::MAX; 4]);
    assert_eq!(
        evaluate(&before, &after, &Limits::default()).cpu_busy_ratio,
        Some(0.75)
    );
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
        let mut after = after_sample();
        after.thermal = Ok(level);
        assert_eq!(
            evaluate(&sample(), &after, &Limits::default()).verdict,
            expected
        );
    }
    let mut after = after_sample();
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
        let mut after = after_sample();
        after.pressure = Some(level);
        assert_eq!(
            evaluate(&sample(), &after, &Limits::default()).verdict,
            expected
        );
    }
    let mut after = after_sample();
    after.pressure = Some(PressureLevel::Warn);
    let limits = Limits {
        max_pressure: PressureLevel::Critical,
        ..Limits::default()
    };
    assert_eq!(evaluate(&sample(), &after, &limits).verdict, Verdict::Admit);
}

#[test]
fn swapping_can_be_allowed_without_hiding_the_delta() -> TestResult {
    let mut after = after_sample();
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
    let mut after = after_sample();
    after.thermal = Ok(ThermalLevel::Sleeping);
    after.pressure = Some(PressureLevel::Critical);
    vm(&mut after)?.swapouts = 25;
    after.cpu_ticks = Ok([75, 10, 5, 10]);
    after.load = Ok([32.0, 0.0, 0.0]);
    after.disk_free_bytes = Ok(BYTES_PER_GIB / 2);
    let report = evaluate(&sample(), &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Refuse);
    assert_eq!(
        report.reasons,
        [
            "thermal sleeping >= heavy",
            "memory pressure critical >= warn",
            "swapouts increased by 3",
            "cpu busy 0.95 > 0.90",
            "disk free 0.50 GiB < 150.00 GiB",
        ]
    );
    Ok(())
}

#[test]
fn disk_threshold_refuses_below_the_minimum_and_is_configurable() {
    let minimum = Limits::default().min_free_disk_bytes;
    assert_eq!(minimum, 150 * BYTES_PER_GIB);
    for (free, expected) in [
        (0, Verdict::Refuse),
        (minimum - 1, Verdict::Refuse),
        (minimum, Verdict::Admit),
        (minimum + 1, Verdict::Admit),
        (u64::MAX, Verdict::Admit),
    ] {
        let mut after = after_sample();
        after.disk_free_bytes = Ok(free);
        let report = evaluate(&sample(), &after, &Limits::default());
        assert_eq!(report.verdict, expected, "free {free}");
        assert_eq!(report.disk_free_bytes, Some(free));
    }
    let mut after = after_sample();
    after.disk_free_bytes = Ok(149 * BYTES_PER_GIB);
    assert_eq!(
        evaluate(&sample(), &after, &Limits::default()).reasons,
        ["disk free 149.00 GiB < 150.00 GiB"]
    );
    let limits = Limits {
        min_free_disk_bytes: 149 * BYTES_PER_GIB,
        ..Limits::default()
    };
    assert_eq!(evaluate(&sample(), &after, &limits).verdict, Verdict::Admit);
    let limits = Limits {
        min_free_disk_bytes: 500 * BYTES_PER_GIB,
        ..Limits::default()
    };
    let report = evaluate(&sample(), &after_sample(), &limits);
    assert_eq!(report.verdict, Verdict::Refuse);
    assert_eq!(report.reasons, ["disk free 400.00 GiB < 500.00 GiB"]);
}

#[test]
fn disk_rule_reads_the_latest_sample() {
    let mut before = sample();
    before.disk_free_bytes = Ok(0);
    let report = evaluate(&before, &after_sample(), &Limits::default());
    assert_eq!(report.verdict, Verdict::Admit);
    assert_eq!(report.disk_free_bytes, Some(400 * BYTES_PER_GIB));
}

#[test]
fn zero_minimum_disables_the_disk_rule() -> TestResult {
    let limits = Limits {
        min_free_disk_bytes: 0,
        ..Limits::default()
    };
    let mut after = after_sample();
    after.disk_free_bytes = Ok(0);
    let report = evaluate(&sample(), &after, &limits);
    assert_eq!(report.verdict, Verdict::Admit);
    assert_eq!(report.disk_free_bytes, Some(0));

    let mut before = sample();
    before.disk_free_bytes = Err(denied());
    after.disk_free_bytes = Err(denied());
    let report = evaluate(&before, &after, &limits);
    assert_eq!(report.verdict, Verdict::Admit);
    assert_eq!(report.reasons, Vec::<String>::new());
    assert_eq!(report.disk_free_bytes, None);
    let mut output = Vec::new();
    crate::render::write(&mut output, &report, Format::Text)?;
    let text = String::from_utf8(output)?;
    assert!(
        text.lines()
            .next()
            .is_some_and(|line| line.ends_with(", disk free null GiB"))
    );
    assert!(text.ends_with("\ndisk_free_bytes: null\n"));
    Ok(())
}

#[test]
fn unreadable_disk_is_unknown_while_the_rule_is_enabled() -> TestResult {
    for first_missing in [true, false] {
        let mut before = sample();
        let mut after = after_sample();
        let phase = if first_missing {
            before.disk_free_bytes = Err(denied());
            "first"
        } else {
            after.disk_free_bytes = Err(denied());
            "second"
        };
        let report = evaluate(&before, &after, &Limits::default());
        assert_eq!(report.verdict, Verdict::Unknown, "first={first_missing}");
        assert_eq!(
            report.reasons,
            [format!(
                "{phase} free disk space unavailable: injected read: Operation not permitted \
                 (os error 1)"
            )]
        );
    }
    let mut after = after_sample();
    after.disk_free_bytes = Err(denied());
    after.thermal = Ok(ThermalLevel::Heavy);
    let report = evaluate(&sample(), &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Unknown);
    assert_eq!(report.disk_free_bytes, None);
    let value = serde_json::to_value(report)?;
    assert_eq!(value.get("disk_free_bytes"), Some(&serde_json::Value::Null));
    Ok(())
}

#[test]
fn disk_reader_reports_a_real_volume_and_names_an_unreadable_path() -> TestResult {
    disk_free_bytes(Some(Path::new("/")))?;

    let missing = Path::new("/nonexistent/headroom-disk-rule-test");
    let Err(error) = disk_free_bytes(Some(missing)) else {
        return Err("a missing path must not produce a free-space reading".into());
    };
    assert_eq!(
        error.to_string(),
        "statfs /nonexistent/headroom-disk-rule-test: No such file or directory (os error 2)"
    );

    let embedded_nul = Path::new(OsStr::from_bytes(b"/tmp\0/x"));
    let Err(error) = disk_free_bytes(Some(embedded_nul)) else {
        return Err("a path with a NUL byte must not reach statfs".into());
    };
    assert_eq!(error.to_string(), "disk path contains a NUL byte");
    Ok(())
}

#[test]
fn unreadable_disk_path_makes_the_default_rule_unknown() {
    let missing = Path::new("/nonexistent/headroom-disk-rule-test");
    let mut after = after_sample();
    after.disk_free_bytes = disk_free_bytes(Some(missing));
    let report = evaluate(&sample(), &after, &Limits::default());
    assert_eq!(report.verdict, Verdict::Unknown);
    assert_eq!(report.verdict.code(), 2);
    assert_eq!(
        report.reasons,
        [
            "second free disk space unavailable: statfs /nonexistent/headroom-disk-rule-test: \
             No such file or directory (os error 2)"
        ]
    );
}

#[test]
fn available_disk_bytes_use_checked_arithmetic() -> TestResult {
    assert_eq!(available_bytes(0, 4096)?, 0);
    assert_eq!(available_bytes(262_144, 4096)?, BYTES_PER_GIB);
    assert_eq!(available_bytes(u64::MAX, 1)?, u64::MAX);
    let Err(error) = available_bytes(u64::MAX, 2) else {
        return Err("an overflowing byte count must be rejected".into());
    };
    assert_eq!(error.to_string(), "free disk byte count overflow");
    assert!(available_bytes(1 << 52, 4096).is_err());
    Ok(())
}

#[test]
fn default_disk_path_needs_a_non_empty_home() -> TestResult {
    assert_eq!(
        home_directory(Some("/Users/worker".into()))?,
        Path::new("/Users/worker")
    );
    for home in [None, Some(OsString::new())] {
        let Err(error) = home_directory(home) else {
            return Err("an unset or empty HOME must not yield a disk path".into());
        };
        assert_eq!(
            error.to_string(),
            "home directory unavailable; pass --disk-path"
        );
    }
    Ok(())
}

#[test]
fn either_required_sample_being_unavailable_is_unknown() {
    for first_missing in [true, false] {
        for field in 0..5 {
            let mut before = sample();
            let mut after = after_sample();
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
    let mut after = after_sample();
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
        let mut after = after_sample();
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
    let mut after = after_sample();
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
    use std::mem::{align_of, offset_of, size_of};

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
    assert_eq!(libc::HOST_CPU_LOAD_INFO, 3);
    assert_eq!(libc::HOST_CPU_LOAD_INFO_COUNT, 4);
    assert_eq!(libc::CPU_STATE_MAX, 4);
    assert_eq!(
        [
            libc::CPU_STATE_USER,
            libc::CPU_STATE_SYSTEM,
            libc::CPU_STATE_IDLE,
            libc::CPU_STATE_NICE,
        ],
        [0, 1, 2, 3]
    );
    assert_eq!(size_of::<libc::host_cpu_load_info>(), 16);
    assert_eq!(align_of::<libc::host_cpu_load_info>(), 4);
    assert_eq!(offset_of!(libc::host_cpu_load_info, cpu_ticks), 0);
    assert_eq!(size_of::<libc::statfs>(), 2168);
    assert_eq!(align_of::<libc::statfs>(), 8);
    assert_eq!(offset_of!(libc::statfs, f_bsize), 0);
    assert_eq!(offset_of!(libc::statfs, f_bavail), 24);
}

#[test]
fn text_report_has_the_documented_keys_order_and_precision() -> TestResult {
    let report = evaluate(&sample(), &after_sample(), &Limits::default());
    let mut output = Vec::new();
    crate::render::write(&mut output, &report, Format::Text)?;
    assert_eq!(
        String::from_utf8(output)?,
        concat!(
            "ADMIT: cpu busy 0.75, load 4.0 on 16 cores, memory free 0.50, thermal nominal, ",
            "disk free 400.0 GiB\n",
            "cpu_busy_ratio: 0.75\n",
            "load1_per_core: 0.25\nload1: 4.0\nload5: 6.0\nload15: 12.0\n",
            "ncpu: 16\nmem_total_bytes: 16384000\nmem_free_ratio: 0.50\n",
            "compressor_bytes: 1638400\nswapouts_delta: 0\nswap_used_bytes: null\n",
            "pressure_level: null\nthermal_level: nominal\n",
            "disk_free_bytes: 429496729600\n",
        )
    );
    Ok(())
}

#[test]
fn json_report_is_one_flat_versioned_object_with_exact_integers() -> TestResult {
    let mut after = after_sample();
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
            "schema": 2, "verdict": "ADMIT", "reasons": [],
            "cpu_busy_ratio": 0.75,
            "load1_per_core": 0.25, "load1": 4.0, "load5": 6.0, "load15": 12.0,
            "ncpu": 16, "mem_total_bytes": 16_384_000, "mem_free_ratio": 0.5,
            "compressor_bytes": 1_638_400, "swapouts_delta": 0,
            "swap_used_bytes": 9_007_199_254_740_993_u64,
            "pressure_level": "normal", "thermal_level": "nominal",
            "disk_free_bytes": 429_496_729_600_u64,
        })
    );
    Ok(())
}

#[test]
fn json_report_appends_free_disk_bytes_after_the_existing_fields() -> TestResult {
    let mut after = after_sample();
    after.disk_free_bytes = Ok(9_007_199_254_740_993);
    let report = evaluate(&sample(), &after, &Limits::default());
    let mut output = Vec::new();
    crate::render::write(&mut output, &report, Format::Json)?;
    let text = String::from_utf8(output)?;
    assert!(text.starts_with(r#"{"schema":2,"verdict":"ADMIT","reasons":[],"cpu_busy_ratio":"#));
    assert!(
        text.ends_with("\"thermal_level\":\"nominal\",\"disk_free_bytes\":9007199254740993}\n")
    );

    after.disk_free_bytes = Ok(BYTES_PER_GIB);
    let report = evaluate(&sample(), &after, &Limits::default());
    let value = serde_json::to_value(report)?;
    assert_eq!(value.get("verdict"), Some(&json!("REFUSE")));
    assert_eq!(
        value.get("reasons"),
        Some(&json!(["disk free 1.00 GiB < 150.00 GiB"]))
    );
    assert_eq!(value.get("disk_free_bytes"), Some(&json!(BYTES_PER_GIB)));
    Ok(())
}

#[test]
fn unknown_output_keeps_missing_fields_null() -> TestResult {
    let mut after = after_sample();
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
    let mut after = after_sample();
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
fn verdict_text_identifies_the_load_fallback() -> TestResult {
    let mut after = after_sample();
    after.cpu_ticks = Err(denied());
    for (load, thermal, expected) in [
        (
            Ok([4.0, 6.0, 12.0]),
            Ok(ThermalLevel::Nominal),
            "ADMIT: cpu busy null, load 4.0 on 16 cores, memory free 0.50, thermal nominal, disk free 400.0 GiB; cpu ticks unavailable, using load fallback",
        ),
        (
            Ok([32.0, 0.0, 0.0]),
            Ok(ThermalLevel::Nominal),
            "REFUSE: load per core 2.00 > 1; cpu ticks unavailable, using load fallback",
        ),
        (
            Ok([4.0, 6.0, 12.0]),
            Err(ReadError::Invalid("injected failure")),
            "UNKNOWN: second thermal state unavailable: injected failure; cpu ticks unavailable, using load fallback",
        ),
    ] {
        after.load = load;
        after.thermal = thermal;
        let report = evaluate(&sample(), &after, &Limits::default());
        let mut output = Vec::new();
        crate::render::write(&mut output, &report, Format::Text)?;
        let text = String::from_utf8(output)?;
        assert_eq!(text.lines().next(), Some(expected));
        assert!(text.contains("\ncpu_busy_ratio: null\n"));
        let json = serde_json::to_value(report)?;
        assert_eq!(json.get("cpu_busy_ratio"), Some(&serde_json::Value::Null));
    }
    Ok(())
}

#[test]
fn cli_defaults_match_the_rule_defaults() -> TestResult {
    let cli = Cli::try_parse_from(["headroom"])?;
    let limits = Limits::default();
    assert_eq!(cli.format, Format::Text);
    assert!((cli.max_cpu_busy - limits.max_cpu_busy).abs() < f64::EPSILON);
    assert!((cli.max_load_per_core - limits.max_load_per_core).abs() < f64::EPSILON);
    assert_eq!(cli.max_thermal, limits.max_thermal);
    assert_eq!(cli.max_pressure, limits.max_pressure);
    assert_eq!(cli.allow_swapping, limits.allow_swapping);
    assert_eq!(cli.min_free_disk, limits.min_free_disk_bytes);
    assert_eq!(cli.limits().min_free_disk_bytes, 150 * BYTES_PER_GIB);
    assert_eq!(cli.disk_path, None);
    Ok(())
}

#[test]
fn cli_parses_byte_counts_and_binary_size_suffixes() -> TestResult {
    for (size, bytes) in [
        ("0", 0),
        ("1", 1),
        ("161061273600", 150 << 30),
        ("4K", 4 << 10),
        ("4KiB", 4 << 10),
        ("500M", 500 << 20),
        ("500MiB", 500 << 20),
        ("150G", 150 << 30),
        ("150GiB", 150 << 30),
        ("2T", 2 << 40),
        ("2TiB", 2 << 40),
        ("0G", 0),
        ("18446744073709551615", u64::MAX),
        ("16777215T", 16_777_215 << 40),
    ] {
        let cli = Cli::try_parse_from(["headroom", "--min-free-disk", size])?;
        assert_eq!(cli.min_free_disk, bytes, "size {size}");
        assert_eq!(cli.limits().min_free_disk_bytes, bytes, "size {size}");
    }
    Ok(())
}

#[test]
fn cli_rejects_malformed_and_oversized_sizes() {
    for size in [
        "",
        "G",
        "GiB",
        "abc",
        "-1",
        "+5",
        "1.5G",
        "1e3",
        "0x10",
        "150 G",
        " 150G",
        "150g",
        "150gib",
        "150GB",
        "150Gi",
        "150GiBB",
        "150P",
        "150G1",
        "18446744073709551616",
        "16777216T",
        "17179869184G",
    ] {
        let result =
            Cli::try_parse_from(["headroom".to_owned(), format!("--min-free-disk={size}")]);
        assert!(result.is_err(), "size {size:?}");
        if let Err(error) = result {
            assert_eq!(error.exit_code(), 2);
            let message = error.to_string();
            assert!(
                message.contains("expected a byte count or a binary size")
                    || message.contains("size does not fit in a 64-bit byte count"),
                "size {size:?}: {message}"
            );
        }
    }
}

#[test]
fn cli_rejects_unknown_flags_and_invalid_thresholds() {
    for arguments in [
        vec!["headroom", "--unknown"],
        vec!["headroom", "positional"],
        vec!["headroom", "--format", "xml"],
        vec!["headroom", "--max-cpu-busy=NaN"],
        vec!["headroom", "--max-cpu-busy=inf"],
        vec!["headroom", "--max-cpu-busy=-0.1"],
        vec!["headroom", "--max-cpu-busy=1.1"],
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
        "--max-cpu-busy",
        "0.8",
        "--max-load-per-core",
        "2.5",
        "--max-thermal",
        "sleeping",
        "--max-pressure",
        "critical",
        "--allow-swapping",
        "--min-free-disk",
        "20GiB",
        "--disk-path",
        "/Volumes/Build",
    ])?;
    assert_eq!(cli.min_free_disk, 20 * BYTES_PER_GIB);
    assert_eq!(cli.disk_path.as_deref(), Some(Path::new("/Volumes/Build")));
    assert_eq!(cli.format, Format::Json);
    assert_eq!(cli.max_thermal, ThermalLevel::Sleeping);
    assert_eq!(cli.max_pressure, PressureLevel::Critical);
    assert!(cli.allow_swapping);
    assert!((cli.max_cpu_busy - 0.8).abs() < f64::EPSILON);
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
