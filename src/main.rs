// Genshin-OS Main Entry Point

use genshin_os::{Shell, LockedBus};
use genshin_os::services::process::ProcessService;
use genshin_os::services::file::FileService;
use genshin_os::hardware::{PhysicalMemory, MMU};
use std::sync::Arc;
use std::thread;

fn main() {
    println!("Initializing Genshin-OS microkernel simulation...");

    let hw_memory = PhysicalMemory::new(4 * 1024 * 1024);
    let mmu = Arc::new(MMU::new(hw_memory.clone(), 4096));
    println!("\u{2713} Hardware (4MB RAM, 4KB pages)");

    let bus = Arc::new(LockedBus::new());
    println!("\u{2713} Message bus");

    let process_bus = bus.clone();
    let proc_mem = hw_memory.clone();
    let proc_mmu = mmu.clone();
    let _process_handle = thread::spawn(move || {
        let service = ProcessService::new(process_bus, proc_mem, proc_mmu);
        service.run();
    });
    println!("\u{2713} Process service (with VirtualCPU)");

    let file_bus = bus.clone();
    let _file_handle = thread::spawn(move || {
        let service = FileService::new(file_bus, 256, 1024 * 1024);
        service.run();
    });
    println!("\u{2713} File service");

    let mut shell = Shell::new(bus);
    println!("\u{2713} Shell");
    println!();

    shell.run_interactive();
}
