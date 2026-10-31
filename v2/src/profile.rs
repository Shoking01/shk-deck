//! Profile model: the same file and schema the Electron host used, so v2 can
//! read the user's existing assignments without a migration.
//!
//! Read-only in this slice: v2 loads `host/profile.json`, it never writes it.

use serde::Deserialize;
use serde_json::Value;
use std::path::Path;

pub const COLUMNS: usize = 5;
pub const ROWS: usize = 3;

pub fn key_index_of(row: usize, column: usize) -> usize {
    row * COLUMNS + column + 1
}

#[derive(Clone, Debug, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub instances: Vec<Instance>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Instance {
    #[serde(default)]
    pub plugin: String,
    #[serde(default)]
    pub action: String,
    pub column: usize,
    pub row: usize,
    #[serde(default)]
    #[allow(dead_code)]
    pub settings: Value,
    #[serde(default, rename = "titleColor")]
    pub title_color: Option<String>,
}

impl Profile {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Self { instances: Vec::new() })
    }

    pub fn instance_at(&self, key_index: usize) -> Option<&Instance> {
        self.instances
            .iter()
            .find(|instance| instance.row < ROWS && instance.column < COLUMNS && key_index_of(instance.row, instance.column) == key_index)
    }
}
