// Genshin-OS Main Entry Point

use genshin_os::hardware::{MMU, PhysicalMemory, Timer, TimerConfig};
use genshin_os::services::device::DeviceService;
use genshin_os::services::file::FileService;
use genshin_os::services::kernel::Kernel;
use genshin_os::services::memory::MemoryService;
use genshin_os::services::process::ProcessService;
use genshin_os::{LockedBus, Shell};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("Initializing Genshin-OS microkernel simulation...");

    let hw_memory = PhysicalMemory::new(4 * 1024 * 1024);
    let mmu = Arc::new(MMU::new(hw_memory.clone(), 4096));
    let bus = Arc::new(LockedBus::new());
    println!("\u{2713} Hardware + Message bus");

    // Kernel MUST subscribe to bus before Timer starts sending
    let (kernel, prx, irx, mrx, frx) = Kernel::new(bus.clone());
    let kernel_handle = thread::spawn(move || {
        kernel.run();
    });
    println!("\u{2713} Kernel");

    // Start hardware timer AFTER Kernel has subscribed
    let timer = Arc::new(Timer::new(bus.clone(), TimerConfig { tick_interval_ms: 10, auto_start: true }));
    println!("\u{2713} Timer (hardware, 100 Hz)");

    // ProcessService — receives from kernel channel
    let process_bus = bus.clone();
    let proc_mem = hw_memory.clone();
    let proc_mmu = mmu.clone();
    let _process_handle = thread::spawn(move || {
        let service = ProcessService::new(process_bus, proc_mem, proc_mmu, prx, irx);
        service.run();
    });
    println!("\u{2713} Process service");

    // MemoryService — receives from kernel channel
    let mem_bus = bus.clone();
    let mem_hw = hw_memory.clone();
    let mem_mmu = mmu.clone();
    let _memory_handle = thread::spawn(move || {
        let service = MemoryService::new(mem_bus, mem_hw, mem_mmu, mrx);
        service.run();
    });
    println!("\u{2713} Memory service");

    let file_bus = bus.clone();
    let _file_handle = thread::spawn(move || {
        let service = FileService::new(file_bus, 256, 1024 * 1024, frx);
        service.run();
    });
    println!("\u{2713} File service");

    // DeviceService — subscribes directly to bus (handles Device messages)
    let device_bus = bus.clone();
    let _device_handle = thread::spawn(move || {
        let service = DeviceService::new(device_bus);
        service.run();
    });
    println!("\u{2713} Device service");

    let mut shell = Shell::new(bus, timer);
    println!("\u{2713} Shell\n");
    shell.run_interactive();
}
