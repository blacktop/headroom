// Compile with: xcrun clang -fsyntax-only -x objective-c tests/sdk_contract.m
// This checks SDK definitions without observing or changing host load.
#import <Foundation/NSProcessInfo.h>
#include <libkern/OSThermalNotification.h>
#include <mach/host_info.h>
#include <mach/machine.h>
#include <mach/vm_statistics.h>
#include <stddef.h>

_Static_assert(kOSThermalPressureLevelNominal == 0, "nominal ABI");
_Static_assert(kOSThermalPressureLevelModerate == 1, "moderate ABI");
_Static_assert(kOSThermalPressureLevelHeavy == 2, "heavy ABI");
_Static_assert(kOSThermalPressureLevelTrapping == 3, "trapping ABI");
_Static_assert(kOSThermalPressureLevelSleeping == 4, "sleeping ABI");
_Static_assert(NSProcessInfoThermalStateNominal == 0, "normal semantics");
_Static_assert(NSProcessInfoThermalStateFair == 1, "slightly elevated semantics");
_Static_assert(NSProcessInfoThermalStateSerious == 2, "high temperature semantics");
_Static_assert(NSProcessInfoThermalStateCritical == 3, "must cool semantics");
_Static_assert(HOST_CPU_LOAD_INFO == 3, "CPU flavor ABI");
_Static_assert(HOST_CPU_LOAD_INFO_COUNT == 4, "CPU word count");
_Static_assert(CPU_STATE_MAX == 4, "CPU state count");
_Static_assert(CPU_STATE_USER == 0 && CPU_STATE_SYSTEM == 1 &&
               CPU_STATE_IDLE == 2 && CPU_STATE_NICE == 3, "CPU state order");
_Static_assert(sizeof(natural_t) == 4, "CPU tick width");
_Static_assert(sizeof(host_cpu_load_info_data_t) == 16, "CPU buffer size");
_Static_assert(_Alignof(host_cpu_load_info_data_t) == 4, "CPU buffer alignment");
_Static_assert(offsetof(host_cpu_load_info_data_t, cpu_ticks) == 0, "CPU ticks ABI");
_Static_assert(HOST_VM_INFO64 == 4, "VM flavor ABI");
_Static_assert(offsetof(vm_statistics64_data_t, free_count) == 0, "free ABI");
_Static_assert(offsetof(vm_statistics64_data_t, inactive_count) == 8, "inactive ABI");
_Static_assert(offsetof(vm_statistics64_data_t, purgeable_count) == 88, "purgeable ABI");
_Static_assert(offsetof(vm_statistics64_data_t, speculative_count) == 92, "speculative ABI");
_Static_assert(offsetof(vm_statistics64_data_t, swapins) == 112, "swapins ABI");
_Static_assert(offsetof(vm_statistics64_data_t, swapouts) == 120, "swapouts ABI");
_Static_assert(offsetof(vm_statistics64_data_t, compressor_page_count) == 128, "compressor ABI");
_Static_assert(offsetof(vm_statistics64_data_t, total_uncompressed_pages_in_compressor) == 144, "VM rev1 tail ABI");
_Static_assert(HOST_VM_INFO64_REV1_COUNT == 38, "VM rev1 word count");
