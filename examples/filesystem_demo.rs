// Demonstration of the shell filesystem functionality
//
// This example shows that cd, pwd, ls, and mkdir now work with
// a real virtual filesystem instead of being "fake" commands.

use genshin_os::ui::shell::filesystem::VirtualFileSystem;

fn main() {
    println!("=== Chao-OS Filesystem Demonstration ===\n");

    // Create a new virtual filesystem
    let mut fs = VirtualFileSystem::new();

    // Test pwd
    println!("Current directory: {}", fs.pwd());

    // Test ls in root directory
    println!("\nContents of root directory:");
    match fs.ls(None) {
        Ok(entries) => {
            for entry in entries {
                println!("  - {}", entry);
            }
        }
        Err(err) => eprintln!("  Error: {}", err),
    }

    // Test cd to /home/user
    println!("\nChanging to /home/user...");
    match fs.cd("/home/user") {
        Ok(_) => println!("✓ Successfully changed directory"),
        Err(err) => eprintln!("✗ Error: {}", err),
    }

    // Test pwd after cd
    println!("Current directory: {}", fs.pwd());

    // Test mkdir
    println!("\nCreating directory 'my_project'...");
    match fs.mkdir("my_project") {
        Ok(_) => println!("✓ Successfully created directory"),
        Err(err) => eprintln!("✗ Error: {}", err),
    }

    // Test ls to see the new directory
    println!("\nContents of current directory:");
    match fs.ls(None) {
        Ok(entries) => {
            if entries.is_empty() {
                println!("  (empty directory)");
            } else {
                for entry in entries {
                    println!("  - {}", entry);
                }
            }
        }
        Err(err) => eprintln!("  Error: {}", err),
    }

    // Test cd to non-existent directory
    println!("\nTrying to cd to non-existent directory...");
    match fs.cd("/nonexistent") {
        Ok(_) => println!("✓ Successfully changed directory"),
        Err(err) => println!("✗ Expected error: {}", err),
    }

    // Test relative path navigation
    println!("\nTesting relative path navigation from /home/user:");
    fs.cd("/home/user").unwrap(); // Reset to known location
    println!("Current directory: {}", fs.pwd());

    match fs.cd("..") {
        Ok(_) => println!("✓ Successfully moved to parent directory"),
        Err(err) => eprintln!("✗ Error: {}", err),
    }
    println!("Current directory: {}", fs.pwd());

    // Test mkdir with absolute path
    println!("\nCreating directory with absolute path /tmp/new_folder...");
    match fs.mkdir("/tmp/new_folder") {
        Ok(_) => println!("✓ Successfully created directory"),
        Err(err) => eprintln!("✗ Error: {}", err),
    }

    // Test ls on specific directory
    println!("\nContents of /tmp:");
    match fs.ls(Some("/tmp")) {
        Ok(entries) => {
            if entries.is_empty() {
                println!("  (empty directory)");
            } else {
                for entry in entries {
                    println!("  - {}", entry);
                }
            }
        }
        Err(err) => eprintln!("  Error: {}", err),
    }

    println!("\n=== Demonstration Complete ===");
}
