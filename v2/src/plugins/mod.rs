//! Plugin runtime: handles third-party plugins
//!
//! By default, runs in "native-only" mode (no Electron required):
//! - Native plugins (.exe) run directly
//! - Image-based plugins (clock, calendar) will use static images
//!
//! To enable HTML plugin support, Electron must be installed and
//! PLUGIN_WEBSOCKET_SERVER must be enabled.

use crate::profile::{COLUMNS, ROWS, Profile, key_index_of};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub mod server;

/// Device identity reported to plugins
pub const DEVICE_ID: &str = "SS550V3";

pub struct PluginRuntime {
    pub server: Option<server::PluginServer>,
    bindings: Vec<server::Binding>,
    by_context: BTreeMap<String, usize>,
}

impl PluginRuntime {
    pub fn start(profile: &Profile) -> Self {
        let bindings = build_bindings(profile);
        let by_context = bindings
            .iter()
            .map(|binding| (binding.context.clone(), key_index_of(binding.row, binding.column)))
            .collect();
        
        // Check if Electron is available
        let electron_available = check_electron_available();
        
        // Enable WebSocket server only if Electron is available AND there are HTML plugins
        let has_html_plugins = bindings.iter().any(|b| !is_native_plugin(&b.plugin));
        
        let server = if has_html_plugins && electron_available {
            println!("[v2] WebSocket server habilitado (Electron disponible)");
            Some(server::PluginServer::start(bindings.clone(), DEVICE_ID.to_string()))
        } else {
            if has_html_plugins {
                println!("[v2] Advertencia: Plugins HTML encontrados pero Electron no disponible - usando imágenes estáticas");
            }
            None
        };
        
        // Launch all plugins
        for binding in bindings.iter() {
            launch_plugin(binding);
        }
        
        Self {
            server,
            bindings,
            by_context,
        }
    }

    pub fn dispatch_key(&self, raw_key_id: usize, event: &'static str) {
        let Some(key_index) = remap_key_id(raw_key_id) else { return };
        let column = (key_index - 1) % COLUMNS;
        let row = (key_index - 1) / COLUMNS;

        for binding in self
            .bindings
            .iter()
            .filter(|binding| binding.column == column && binding.row == row)
        {
            if event == "keyDown" {
                launch_bound_app(&binding.settings);
            }
        }
    }

    pub fn key_of_context(&self, context: &str) -> Option<usize> {
        self.by_context.get(context).copied()
    }
}

/// Launch the app named by settings.path (or appPath) — launcher-style action.
fn launch_bound_app(settings: &Value) {
    let app_path = settings
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| settings.get("appPath").and_then(Value::as_str));
    let Some(app_path) = app_path else { return };
    let expanded = expand_env(app_path);
    // .lnk shortcuts and documents need shell semantics, not a direct exec.
    let is_shell_target = expanded.to_ascii_lowercase().ends_with(".lnk")
        || expanded.to_ascii_lowercase().ends_with(".url");
    let result = if is_shell_target {
        Command::new("cmd")
            .args(["/c", "start", "", &expanded])
            .spawn()
    } else {
        Command::new(&expanded).spawn()
    };
    match result {
        Ok(_) => eprintln!("[v2] lanzada: {expanded}"),
        Err(error) => eprintln!("[v2] error lanzando {expanded}: {error}"),
    }
}

/// Expand %ENV% variables in a path (Windows-style), no shell dependency.
fn expand_env(path: &str) -> String {
    let mut result = path.to_string();
    if let Ok(home) = std::env::var("USERPROFILE") {
        result = result.replace("%USERPROFILE%", &home);
    }
    for key in ["APPDATA", "LOCALAPPDATA", "PROGRAMFILES", "PROGRAMFILES(X86)", "WINDIR", "USERNAME"] {
        if let Ok(value) = std::env::var(key) {
            let pattern = format!("%{key}%");
            result = result.replace(&pattern, &value);
        }
    }
    result
}

fn check_electron_available() -> bool {    let paths = [
        r"C:\Program Files (x86)\electron\electron.exe",
        r"C:\Program Files\electron\electron.exe",
        r"C:\Users\%USERNAME%\AppData\Local\electron\electron.exe",
    ];
    
    for path in &paths {
        if PathBuf::from(path).exists() {
            println!("[v2] Electron encontrado en: {}", path);
            return true;
        }
    }
    
    // Check PATH
    if let Ok(_) = Command::new("electron.exe").arg("--version").output() {
        return true;
    }
    
    false
}

fn launch_plugin(binding: &server::Binding) {
    let plugin = &binding.plugin;
    
    if is_native_plugin(plugin) {
        println!("[v2] lanzando plugin nativo: {}", plugin);
        if let Some(dir) = get_plugin_dir(plugin) {
            if let Ok(content) = std::fs::read_to_string(dir.join("manifest.json")) {
                if let Ok(manifest) = serde_json::from_str::<Value>(&content) {
                    if let Some(entry) = manifest.get("CodePathWin").and_then(Value::as_str) {
                        let entry_path = dir.join(entry);
                        if entry_path.exists() {
                            if let Ok(json) = serde_json::to_string(&manifest) {
                                println!("[v2] lanzando: {:?}", entry_path);
                                let _ = Command::new(&entry_path)
                                    .arg(json)
                                    .stdin(Stdio::null())
                                    .stdout(Stdio::inherit())
                                    .stderr(Stdio::inherit())
                                    .spawn();
                            }
                        }
                    }
                }
            }
        }
    } else {
        println!("[v2] plugin HTML (requiere Electron): {}", plugin);
    }
}

fn is_native_plugin(plugin: &str) -> bool {
    plugin.starts_with("com.inginversa")
}

fn get_plugin_dir(plugin: &str) -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|base| {
        PathBuf::from(base)
            .join("HotSpot")
            .join("StreamDeck")
            .join("plugins")
            .join(format!("{}.sdPlugin", plugin))
    })
}

pub fn remap_key_id(raw_key_id: usize) -> Option<usize> {
    if raw_key_id == 0 {
        return None;
    }
    let raw_row = (raw_key_id - 1) / COLUMNS;
    let column = (raw_key_id - 1) % COLUMNS;
    if raw_row >= ROWS {
        return None;
    }
    Some((ROWS - 1 - raw_row) * COLUMNS + column + 1)
}

fn build_bindings(profile: &Profile) -> Vec<server::Binding> {
    profile
        .instances
        .iter()
        .filter_map(|instance| {
            if instance.plugin.is_empty() || instance.action.is_empty() {
                return None;
            }
            if instance.column >= COLUMNS || instance.row >= ROWS {
                return None;
            }
            let key_index = key_index_of(instance.row, instance.column);
            Some(server::Binding {
                plugin: instance.plugin.clone(),
                action: instance.action.clone(),
                context: format!("key{key_index}-{}", instance.action),
                column: instance.column,
                row: instance.row,
                settings: instance.settings.clone(),
                title_color: instance
                    .title_color
                    .clone()
                    .unwrap_or_else(|| "#ffffff".to_string()),
            })
        })
        .collect()
}