//! Stream Deck Plugin: Application Launcher
//!
//! Opens applications when keys are pressed.
//! Settings from profile.json:
//! { "settings": { "appPath": "C:\\\\Path\\\\To\\\\App.exe" } }
//!
//! Usage:
//! 1. Add plugin entries in host/profile.json with app paths
//! 2. Run reard-v2.exe (auto-discovers and launches plugins)
//! 3. Press keys to open applications

use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;
use std::thread;

/// Plugin metadata passed as command line argument
#[derive(Debug, Deserialize)]
struct Manifest {
    uuid: String,
    name: String,
    version: String,
}

/// Settings for each key context
struct KeySettings {
    app_path: String,
    working_dir: Option<String>,
    icon_base64: Option<String>,
}

/// In-memory store for key settings (would be populated from host messages)
struct PluginState {
    settings: Mutex<std::collections::HashMap<String, KeySettings>>,
}

impl PluginState {
    fn new() -> Self {
        Self {
            settings: Mutex::new(std::collections::HashMap::new()),
        }
    }
    
    fn register_key(&self, context: &str, app_path: &str) {
        let mut settings = self.settings.lock().unwrap();
        settings.insert(
            context.to_string(),
            KeySettings {
                app_path: app_path.to_string(),
                working_dir: None,
                icon_base64: None,
            },
        );
    }
    
    fn launch_app(&self, context: &str) {
        let settings = self.settings.lock().unwrap();
        if let Some(key_settings) = settings.get(context) {
            let app_path = &key_settings.app_path;
            let working_dir = key_settings.working_dir.as_ref();
            
            println!("[launcher] Opening: {}", app_path);
            
            #[cfg(windows)]
            {
                let result = Command::new(app_path)
                    .current_dir(working_dir)
                    .spawn();
                
                match result {
                    Ok(_) => {
                        println!("[launcher] App launched successfully");
                    }
                    Err(e) => {
                        eprintln!("[launcher] Failed to launch app: {}", e);
                    }
                }
            }
            
            #[cfg(not(windows))]
            {
                let result = Command::new(app_path)
                    .current_dir(working_dir.as_deref())
                    .spawn();
                
                match result {
                    Ok(_) => {
                        println!("[launcher] App launched successfully");
                    }
                    Err(e) => {
                        eprintln!("[launcher] Failed to launch app: {}", e);
                    }
                }
            }
        }
    }
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

    println!("[launcher] Starting plugin: {} (v{})", manifest.name, manifest.version);
    
    let state = PluginState::new();
    
    // Read messages from stdin (WebSocket events from host)
    let reader = BufReader::new(std::io::stdin().lock());
    
    for line in reader.lines() {
        match line {
            Ok(msg) => {
                // Parse as JSON to get the event
                if let Ok(value) = serde_json::from_str::<Value>(&msg) {
                    let event = value.get("event").and_then(|e| e.as_str());
                    let context = value.get("context").and_then(|c| c.as_str());
                    let payload = value.get("payload");
                    
                    match event {
                        Some("willAppear") => {
                            // Extract settings for this key
                            if let Some(ctx) = context {
                                if let Some(settings) = payload.and_then(|p| p.get("settings")) {
                                    if let Some(app_path) = settings.get("appPath").and_then(|a| a.as_str()) {
                                        state.register_key(ctx, app_path);
                                        println!("[launcher] Registered key {} -> {}", ctx, app_path);
                                        
                                        // Send initial image (button icon)
                                        // In a real implementation, you'd generate a proper icon
                                        println!("[launcher] Would send icon for {}", app_path);
                                    }
                                }
                            }
                        }
                        Some("keyDown") => {
                            if let Some(ctx) = context {
                                println!("[launcher] Key pressed: {}", ctx);
                                state.launch_app(ctx);
                            }
                        }
                        _ => {}
                    }
                }
            }
            Err(_) => break, // EOF
        }
    }
    
    println!("[launcher] Plugin shutting down");
}