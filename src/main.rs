// Genshin-OS Main Entry Point

use genshin_os::{Shell, LockedBus};
use genshin_os::services::kernel::Kernel;
use genshin_os::services::process::ProcessService;
use genshin_os::services::file::FileService;
use genshin_os::hardware::{PhysicalMemory, MMU};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("Initializing Genshin-OS microkernel simulation...");

    let hw_memory = PhysicalMemory::new(4 * 1024 * 1024);
    let mmu = Arc::new(MMU::new(hw_memory.clone(), 4096));
    let bus = Arc::new(LockedBus::new());
    println!("\u{2713} Hardware + Message bus");

    // Kernel owns the bus subscription, creates service channels
    let (kernel, prx, mrx, frx) = Kernel::new(bus.clone());
    let kernel_handle = thread::spawn(move || { kernel.run(); });
    println!("\u{2713} Kernel");

    // ProcessService — receives from kernel channel
    let process_bus = bus.clone();
    let proc_mem = hw_memory.clone();
    let proc_mmu = mmu.clone();
    let _process_handle = thread::spawn(move || {
        let service = ProcessService::new(process_bus, proc_mem, proc_mmu, prx);
        service.run();
    });
    println!("\u{2713} Process service");

    // MemoryService is not started — included for future
    // FileService — receives from kernel channel
    let file_bus = bus.clone();
    let _file_handle = thread::spawn(move || {
        let service = FileService::new(file_bus, 256, 1024 * 1024, frx);
        service.run();
    });
    println!("\u{2713} File service");

    let mut shell = Shell::new(bus);
    println!("\u{2713} Shell\n");
    shell.run_interactive();
}
