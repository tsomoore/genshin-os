// Chao-OS Main Entry Point
//
// This is the main entry point for the Chao-OS microkernel simulation.
// It initializes the system components and starts the interactive shell.

use genshin_os::{Shell, LockedBus};
use genshin_os::services::process::ProcessService;
use std::sync::Arc;
use std::thread;

fn main() {
    println!("Initializing Chao-OS microkernel simulation...");

    // Create the message bus - central communication hub
    let bus = Arc::new(LockedBus::new());
    println!("✓ Message bus initialized");

    // Create and start the process service in a background thread
    let process_bus = bus.clone();
    let process_service = thread::spawn(move || {
        let service = ProcessService::new(process_bus);
        service.run();
    });
    println!("✓ Process service started");

    // Initialize and start the shell
    let mut shell = Shell::new(bus);
    println!("✓ Shell initialized");
    println!();

    // Start the interactive shell
    shell.run_interactive();
}
