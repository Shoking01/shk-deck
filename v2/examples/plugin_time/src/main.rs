//! Minimal Stream Deck plugin template (Time Plugin Example)
//! 
//! This example shows how to create a native Rust plugin for REA RD v2.
//! Compile: cargo build --release
//! Run: Will be launched automatically by reard-v2.exe

use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::io::{BufRead, BufReader, Write};
use std::process::Command;
use std::thread;
use std::time::Duration;

/// Plugin manifest passed as command-line argument
#[derive(Debug, Deserialize)]
struct Manifest {
    uuid: String,
    name: String,
    version: String,
}

/// Message from the host
#[derive(Debug, Deserialize)]
struct HostMessage {
    event: String,
    context: String,
    #[serde(default)]
    payload: Value,
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    if args.len() < 2 {
        eprintln!("Usage: {} <manifest-json>", args[0]);
        std::process::exit(1);
    }

    // Parse manifest
    let manifest: Manifest = match serde_json::from_str(&args[1]) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to parse manifest: {}", e);
            std::process::exit(1);
        }
    };

    println!("[time-plugin] Starting: {} (v{})", manifest.name, manifest.version);
    
    // In a real plugin, you would:
    // 1. Find the WebSocket port from environment or discovery
    // 2. Connect to the WebSocket server
    // 3. Send registerPlugin event
    // 4. Listen for willAppear, keyDown, etc.
    // 5. Send setImage, setTitle events

    // Simulate a running plugin that sends a clock image every second
    // This is a simplified example - real plugins need WebSocket connection
    loop {
        println!("[time-plugin] Tick - would send updated image");
        thread::sleep(Duration::from_secs(1));
    }
}