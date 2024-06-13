use opentelemetry::{
    metrics::{Meter, Unit},
    Key,
};
use parking_lot::Mutex;
use sysinfo::{
    get_current_pid, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System,
};

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemcpuutilization
const SYSTEM_CPU_UTILIZATION: &str = "system.cpu.utilization";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemcpuphysicalcount
const SYSTEM_CPU_PHYSICAL_COUNT: &str = "system.cpu.physical.count";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemcpuphysicalcount
const SYSTEM_CPU_LOGICAL_COUNT: &str = "system.cpu.logical.count";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemcpufrequency
const SYSTEM_CPU_FREQUENCY: &str = "system.cpu.frequency";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemmemoryusage
const SYSTEM_MEMORY_USAGE: &str = "system.memory.usage";

// (this is swap)
// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemmemoryutilization
const SYSTEM_MEMORY_UTILIZATION: &str = "system.memory.utilization";

// (this is swap)
// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systempagingusage
const SYSTEM_PAGING_USAGE: &str = "system.paging.usage";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systempagingutilization
const SYSTEM_PAGING_UTILIZATION: &str = "system.paging.utilization";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemnetworkio
const SYSTEM_NETWORK_IO: &str = "system.network.io";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemfilesystemusage
const SYSTEM_FILESYSTEM_USAGE: &str = "system.filesystem.usage";

// https://opentelemetry.io/docs/specs/semconv/system/system-metrics/#metric-systemfilesystemutilization
const SYSTEM_FILESYSTEM_UTILIZATION: &str = "system.filesystem.utilization";

// Used to specify which CPU the metric is for:
const SYSTEM_CPU_LOGICAL_NUMBER: Key = Key::from_static_str("system.cpu.logical_number");

// https://opentelemetry.io/docs/specs/semconv/system/process-metrics/#metric-processcpuutilization
const PROCESS_CPU_UTILIZATION: &str = "process.cpu.utilization";

// https://opentelemetry.io/docs/specs/semconv/system/process-metrics/#metric-processmemoryusage
const PROCESS_MEMORY_USAGE: &str = "process.memory.usage";

// https://opentelemetry.io/docs/specs/semconv/system/process-metrics/#metric-processmemoryvirtual
const PROCESS_MEMORY_VIRTUAL: &str = "process.memory.virtual";

// https://opentelemetry.io/docs/specs/semconv/system/process-metrics/#metric-processdiskio
const PROCESS_DISK_IO: &str = "process.disk.io";

// Used to specify disk info:
const SYSTEM_FILESYSTEM_MOUNTPOINT: Key = Key::from_static_str("system.filesystem.mountpoint");
const SYSTEM_FILESYSTEM_TYPE: Key = Key::from_static_str("system.filesystem.type");

// read/write for disk io, receive/transmit for network io:
const DIRECTION: Key = Key::from_static_str("direction");

// Device name for filesystem and network:
const SYSTEM_DEVICE: Key = Key::from_static_str("system.device");

use super::record_exception;
use crate::prelude::*;

/// Automatically record system metrics:
/// SYSTEM WIDE:
/// - system.cpu.physical.count
/// - system.cpu.logical.count
/// - system.cpu.utilization
/// - system.cpu.frequency
/// - system.memory.usage (RAM)
/// - system.memory.utilization (RAM)
/// - system.paging.usage (swap)
/// - system.paging.utilization (swap)
/// - system.network.io
/// - system.filesystem.usage
/// - system.filesystem.utilization
///
/// CURRENT PROCESS:
/// - process.cpu.utilization
/// - process.memory.usage
/// - process.memory.virtual
/// - process.disk.io
pub fn init_system_and_process_metrics(meter: &Meter) -> RResult<(), AnyErr> {
    let system_cpu_physical_count = meter
        .u64_observable_gauge(SYSTEM_CPU_PHYSICAL_COUNT)
        .with_description("The number of physical CPUs.")
        .init();

    let system_cpu_logical_count = meter
        .u64_observable_gauge(SYSTEM_CPU_LOGICAL_COUNT)
        .with_description("The number of logical CPUs.")
        .init();

    let system_cpu_utilisation = meter
        .f64_observable_gauge(SYSTEM_CPU_UTILIZATION)
        .with_description("The total CPU usage.")
        .with_unit(Unit::new("1"))
        .init();

    let system_cpu_frequency = meter
        .u64_observable_gauge(SYSTEM_CPU_FREQUENCY)
        .with_description("The frequency of the CPU.")
        .with_unit(Unit::new("Hz"))
        .init();

    let system_memory_usage = meter
        .u64_observable_gauge(SYSTEM_MEMORY_USAGE)
        .with_description("The total amount of memory in use.")
        .with_unit(Unit::new("Bytes"))
        .init();

    let system_memory_utilisation = meter
        .f64_observable_gauge(SYSTEM_MEMORY_UTILIZATION)
        .with_description("The total memory usage ratio.")
        .with_unit(Unit::new("1"))
        .init();

    let system_paging_usage = meter
        .u64_observable_gauge(SYSTEM_PAGING_USAGE)
        .with_description("The total amount of swap in use.")
        .with_unit(Unit::new("Bytes"))
        .init();

    let system_paging_utilisation = meter
        .f64_observable_gauge(SYSTEM_PAGING_UTILIZATION)
        .with_description("The total swap usage ratio.")
        .with_unit(Unit::new("1"))
        .init();

    let system_network_io = meter
        .u64_observable_counter(SYSTEM_NETWORK_IO)
        .with_description("Network bytes transferred.")
        .with_unit(Unit::new("Bytes"))
        .init();

    let system_filesystem_usage = meter
        .u64_observable_gauge(SYSTEM_FILESYSTEM_USAGE)
        .with_description("The total data on disk.")
        .with_unit(Unit::new("Bytes"))
        .init();

    let system_filesystem_utilization = meter
        .f64_observable_gauge(SYSTEM_FILESYSTEM_UTILIZATION)
        .with_description("The total disk usage ratio.")
        .with_unit(Unit::new("1"))
        .init();

    let process_cpu_utilization = meter
        .f64_observable_gauge(PROCESS_CPU_UTILIZATION)
        .with_description("The process' CPU usage ratio.")
        .with_unit(Unit::new("1"))
        .init();
    let process_memory_usage = meter
        .u64_observable_gauge(PROCESS_MEMORY_USAGE)
        .with_description("The process' amount of memory in use.")
        .with_unit(Unit::new("Bytes"))
        .init();
    let process_memory_virtual = meter
        .u64_observable_gauge(PROCESS_MEMORY_VIRTUAL)
        .with_description("The amount of committed virtual memory.")
        .with_unit(Unit::new("Bytes"))
        .init();
    let process_disk_io = meter
        .u64_observable_counter(PROCESS_DISK_IO)
        .with_description("Disk bytes transferred.")
        .with_unit(Unit::new("Bytes"))
        .init();

    let pid =
        get_current_pid().map_err(|err| anyerr!("Couldn't get current pid. Error: {}", err))?;

    // Apparently much more efficient + accurate re-using these objects than creating each time in the callback:
    let objs = Mutex::new((
        System::new_all(),
        Networks::new_with_refreshed_list(),
        Disks::new_with_refreshed_list(),
    ));
    meter
        .register_callback(
            &[
                system_cpu_physical_count.as_any(),
                system_cpu_logical_count.as_any(),
                system_cpu_utilisation.as_any(),
                system_cpu_frequency.as_any(),
                system_memory_usage.as_any(),
                system_memory_utilisation.as_any(),
                system_paging_usage.as_any(),
                system_paging_utilisation.as_any(),
                system_network_io.as_any(),
                system_filesystem_usage.as_any(),
                system_filesystem_utilization.as_any(),
                process_cpu_utilization.as_any(),
                process_memory_usage.as_any(),
                process_memory_virtual.as_any(),
                process_disk_io.as_any(),
            ],
            move |context| {
                let mut objs = objs.lock();
                let (sys, networks, disks) = &mut *objs;

                // Refresh everything needed for system metrics:
                sys.refresh_specifics(
                    RefreshKind::new()
                        .with_cpu(CpuRefreshKind::new().with_cpu_usage())
                        .with_memory(MemoryRefreshKind::everything()),
                );

                let physical_core_count = if let Some(count) = sys.physical_core_count() {
                    count as u64
                } else {
                    record_exception("Could not get physical core count. Defaulting to 2.", "");
                    2
                };
                let cpus = sys.cpus();
                let logical_core_count = cpus.len() as u64;

                // system.cpu.physical.count
                context.observe_u64(&system_cpu_physical_count, physical_core_count, &[]);

                // system.cpu.logical.count
                context.observe_u64(&system_cpu_logical_count, logical_core_count, &[]);

                for (cpu_index, cpu) in cpus.iter().enumerate() {
                    // system.cpu.utilization
                    context.observe_f64(
                        &system_cpu_utilisation,
                        (cpu.cpu_usage() as f64) / 100.0,
                        &[SYSTEM_CPU_LOGICAL_NUMBER.i64(cpu_index as i64)],
                    );

                    // system.cpu.frequency
                    context.observe_u64(
                        &system_cpu_frequency,
                        cpu.frequency(),
                        &[SYSTEM_CPU_LOGICAL_NUMBER.i64(cpu_index as i64)],
                    );
                }

                let used_memory = sys.used_memory();

                // system.memory.usage
                context.observe_u64(&system_memory_usage, used_memory, &[]);

                // system.memory.utilization
                context.observe_f64(
                    &system_memory_utilisation,
                    used_memory as f64 / sys.total_memory() as f64,
                    &[],
                );

                // system.paging.usage
                context.observe_u64(&system_paging_usage, sys.used_swap(), &[]);

                // system.paging.utilization
                context.observe_f64(
                    &system_paging_utilisation,
                    sys.used_swap() as f64 / sys.total_swap() as f64,
                    &[],
                );

                // system.network.io
                networks.refresh_list();
                for (interface_name, data) in networks.iter() {
                    let net_common_attributes = [SYSTEM_DEVICE.string(interface_name.to_string())];
                    context.observe_u64(
                        &system_network_io,
                        data.received(),
                        &[
                            net_common_attributes.as_slice(),
                            &[DIRECTION.string("receive")],
                        ]
                        .concat(),
                    );
                    context.observe_u64(
                        &system_network_io,
                        data.transmitted(),
                        &[
                            net_common_attributes.as_slice(),
                            &[DIRECTION.string("transmit")],
                        ]
                        .concat(),
                    );
                }

                // system.filesystem.usage + system.filesystem.utilization
                disks.refresh_list();
                for disk in disks.iter() {
                    let disk_common_attributes = [
                        SYSTEM_DEVICE.string(disk.name().to_string_lossy().to_string()),
                        SYSTEM_FILESYSTEM_MOUNTPOINT
                            .string(disk.mount_point().to_string_lossy().to_string()),
                        SYSTEM_FILESYSTEM_TYPE
                            .string(disk.file_system().to_string_lossy().to_string()),
                    ];
                    let total_space = disk.total_space();
                    context.observe_u64(
                        &system_filesystem_usage,
                        total_space,
                        &disk_common_attributes,
                    );
                    context.observe_f64(
                        &system_filesystem_utilization,
                        (total_space - disk.available_space()) as f64 / total_space as f64,
                        &disk_common_attributes,
                    );
                }

                // Refresh the process for the specific metrics:
                sys.refresh_process(pid);
                if let Some(process) = sys.process(pid) {
                    // process.cpu.utilization
                    context.observe_f64(
                        &process_cpu_utilization,
                        ((process.cpu_usage() / 100.0) / logical_core_count as f32).into(),
                        &[],
                    );

                    // process.memory.usage
                    context.observe_u64(&process_memory_usage, process.memory(), &[]);

                    // process.memory.virtual
                    context.observe_u64(&process_memory_virtual, process.virtual_memory(), &[]);

                    // - process.disk.io
                    let disk_io = process.disk_usage();
                    context.observe_u64(
                        &process_disk_io,
                        disk_io.read_bytes,
                        &[DIRECTION.string("read")],
                    );
                    context.observe_u64(
                        &process_disk_io,
                        disk_io.written_bytes,
                        &[DIRECTION.string("write")],
                    );
                } else {
                    record_exception(
                        "Could not get current process for system metric collection.",
                        "",
                    );
                }
            },
        )
        .change_context(AnyErr)?;
    Ok(())
}
