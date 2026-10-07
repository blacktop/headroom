use std::ffi::CStr;
use std::io;
use std::mem::{MaybeUninit, offset_of, size_of};
use std::ptr;

use mach2::host_info::HOST_VM_INFO64_COUNT;
use mach2::kern_return::KERN_SUCCESS;
use mach2::mach_init::mach_host_self;
use mach2::mach_port::mach_port_deallocate;
use mach2::port::MACH_PORT_NULL;
use mach2::traps::mach_task_self;
use mach2::vm_statistics::vm_statistics64;
use mach2::vm_types::integer_t;

use crate::model::{PressureLevel, ReadError, Sample, ThermalLevel, VmCounters};

// SDK notify.h. These three symbols are public libSystem interfaces.
unsafe extern "C" {
    fn notify_register_check(name: *const libc::c_char, token: *mut libc::c_int) -> u32;
    fn notify_get_state(token: libc::c_int, state: *mut u64) -> u32;
    fn notify_cancel(token: libc::c_int) -> u32;
}

pub(crate) fn take() -> Sample {
    Sample {
        load: load(),
        ncpu: ncpu(),
        mem_total_bytes: mem_total_bytes(),
        vm: vm(),
        thermal: thermal(),
        swap_used_bytes: swap_used_bytes().ok(),
        pressure: pressure().ok(),
    }
}

fn os_error(operation: &'static str) -> ReadError {
    ReadError::Os {
        operation,
        source: io::Error::last_os_error(),
    }
}

fn status(operation: &'static str, code: impl Into<i64>) -> ReadError {
    ReadError::Status {
        operation,
        code: code.into(),
    }
}

/// Read a fixed-size sysctl without allocating or supplying a new value.
///
/// # Safety
/// `T` must exactly match the named sysctl's C output type, and every value the
/// kernel may return must be a valid `T` without references or owned resources.
unsafe fn sysctl_value<T: Copy>(
    name: &'static CStr,
    operation: &'static str,
) -> Result<T, ReadError> {
    let mut value = MaybeUninit::<T>::uninit();
    let mut size = size_of::<T>();
    // SAFETY: The caller establishes the C layout and validity of T. The name
    // is terminated; the aligned, exclusive output and length live through the
    // synchronous C call. Null newp makes this read-only; C cannot unwind.
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            value.as_mut_ptr().cast(),
            &raw mut size,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(os_error(operation));
    }
    if size != size_of::<T>() {
        return Err(ReadError::Invalid("unexpected sysctl output size"));
    }
    // SAFETY: A successful exact-size read initialized T, whose valid bit
    // patterns and layout are part of the caller's contract. Ownership is local.
    Ok(unsafe { value.assume_init() })
}

fn ncpu() -> Result<u32, ReadError> {
    // SAFETY: hw.ncpu returns a C int; all i32 bit patterns are valid.
    let value = unsafe { sysctl_value::<libc::c_int>(c"hw.ncpu", "hw.ncpu") }?;
    u32::try_from(value).map_err(|_| ReadError::Invalid("negative core count"))
}

fn mem_total_bytes() -> Result<u64, ReadError> {
    // SAFETY: hw.memsize returns uint64_t; all u64 bit patterns are valid.
    unsafe { sysctl_value(c"hw.memsize", "hw.memsize") }
}

fn swap_used_bytes() -> Result<u64, ReadError> {
    // SAFETY: libc::xsw_usage is the sys/sysctl.h output layout for vm.swapusage;
    // its fields are plain integers, with no validity or ownership restrictions.
    let usage = unsafe { sysctl_value::<libc::xsw_usage>(c"vm.swapusage", "vm.swapusage") }?;
    Ok(usage.xsu_used)
}

fn pressure() -> Result<PressureLevel, ReadError> {
    // SAFETY: This sysctl returns a C int; all i32 bit patterns are valid.
    let value = unsafe {
        sysctl_value::<libc::c_int>(
            c"kern.memorystatus_vm_pressure_level",
            "kern.memorystatus_vm_pressure_level",
        )
    }?;
    PressureLevel::from_raw(value)
}

fn load() -> Result<[f64; 3], ReadError> {
    let mut values = [0.0; 3];
    // SAFETY: values is an exclusive, aligned buffer for exactly three doubles.
    // getloadavg writes at most the requested count, retains no pointer, and
    // cannot unwind across the C ABI.
    let count = unsafe { libc::getloadavg(values.as_mut_ptr(), 3) };
    if count < 0 {
        Err(os_error("getloadavg"))
    } else if count != 3 {
        Err(ReadError::Invalid("incomplete load averages"))
    } else {
        Ok(values)
    }
}

fn vm() -> Result<VmCounters, ReadError> {
    // The SDK's REV1 ends at swapped_count; all requested fields precede it.
    let minimum =
        u32::try_from(offset_of!(vm_statistics64, swapped_count) / size_of::<integer_t>())
            .map_err(|_| ReadError::Invalid("VM structure count overflow"))?;
    let mut info = vm_statistics64::default();
    let mut count = HOST_VM_INFO64_COUNT;
    // SAFETY: This no-argument Mach trap returns a send right owned by this
    // task; it takes no pointers and cannot unwind across the C ABI.
    let host = unsafe { mach_host_self() };
    if host == MACH_PORT_NULL {
        return Err(ReadError::Invalid("mach_host_self returned a null port"));
    }
    // SAFETY: host is our live send right. mach2's C-layout VM buffer is
    // initialized, aligned, and exclusively borrowed. Its actual integer-word
    // capacity is passed to Mach, preventing writes beyond the older binding.
    // The synchronous call retains neither pointer and cannot unwind.
    let result = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&raw mut info).cast(),
            &raw mut count,
        )
    };
    // SAFETY: mach_task_self is a borrowed name for this task; host is the one
    // owned send-right reference acquired above. Deallocate it exactly once,
    // after the synchronous read, on success and failure. No pointers escape.
    let released = unsafe { mach_port_deallocate(mach_task_self(), host) };
    if result != KERN_SUCCESS {
        return Err(status("host_statistics64", result));
    }
    if released != KERN_SUCCESS {
        return Err(status("mach_port_deallocate", released));
    }
    if count < minimum || count > HOST_VM_INFO64_COUNT {
        return Err(ReadError::Invalid("incomplete VM statistics revision"));
    }
    // SAFETY: libSystem initializes this aligned, process-lifetime page-size
    // scalar before main and never mutates it afterward. Reading creates no
    // reference, ownership transfer, or cross-thread mutation.
    let page_size = unsafe { mach2::vm_page_size::vm_kernel_page_size };
    Ok(VmCounters {
        free_count: u64::from(info.free_count),
        inactive_count: u64::from(info.inactive_count),
        speculative_count: u64::from(info.speculative_count),
        purgeable_count: u64::from(info.purgeable_count),
        compressor_page_count: u64::from(info.compressor_page_count),
        swapins: info.swapins,
        swapouts: info.swapouts,
        page_size: u64::try_from(page_size)
            .map_err(|_| ReadError::Invalid("kernel page size overflow"))?,
    })
}

fn thermal() -> Result<ThermalLevel, ReadError> {
    let mut token = 0;
    // SAFETY: The static C string is terminated; token is a live, aligned,
    // exclusive int output. notify retains no output pointer and cannot unwind.
    let registered = unsafe {
        notify_register_check(
            c"com.apple.system.thermalpressurelevel".as_ptr(),
            &raw mut token,
        )
    };
    if registered != 0 {
        return Err(status("notify_register_check", registered));
    }
    let mut state = 0;
    // SAFETY: token is our successful registration. state is an exclusive u64
    // output valid through the synchronous call; no pointer escapes or unwinds.
    let result = unsafe { notify_get_state(token, &raw mut state) };
    // SAFETY: This is the single cancellation of our live registration, after
    // its last use. notify_cancel takes no pointers and cannot unwind.
    let cancelled = unsafe { notify_cancel(token) };
    if result != 0 {
        return Err(status("notify_get_state", result));
    }
    if cancelled != 0 {
        return Err(status("notify_cancel", cancelled));
    }
    ThermalLevel::from_raw(state)
}
