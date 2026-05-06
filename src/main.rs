// Chao-OS Main Entry Point
//
// This is the main entry point for the Chao-OS microkernel simulation.
// It initializes the system components and starts the interactive shell.

use genshin_os::{Shell, LockedBus};
use std::sync::Arc;

fn main() {
    println!("Initializing Chao-OS microkernel simulation...");

    // Create the message bus - central communication hub
    let bus = Arc::new(LockedBus::new());
    println!("✓ Message bus initialized");

    // Initialize and start the shell
    let mut shell = Shell::new(bus);
    println!("✓ Shell initialized");
    println!();

    // Start the interactive shell
    shell.run_interactive();
}
