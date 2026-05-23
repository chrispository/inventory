//! Cargo collector. Lists crates installed via `cargo install` — i.e. binaries
//! living under `$CARGO_HOME/bin`. Cargo does not track per-crate install dates,
//! so we approximate by stat'ing `.crates.toml`.

use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::path::Path;
use std::process::Command;

/// Crates installed via `cargo install`. `cargo install --list` formats one
/// crate per "name vX.Y.Z:" header line; we ignore the indented binary lines
/// that follow each header. There's no per-crate install date in cargo's
/// metadata, so we approximate using the mtime of `.crates.toml` — meaning
/// every crate shares the date of the most recent install.
pub struct CargoCollector;

impl Collector for CargoCollector {
    fn enabled(&self) -> bool {
        Command::new("cargo").arg("--version").output().is_ok()
    }

    fn collect(&self) -> Vec<Package> {
        let output = match Command::new("cargo").args(["install", "--list"]).output() {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut packages = Vec::new();

        let cargo_home = std::env::var("CARGO_HOME")
            .ok()
            .or_else(dirs_fallback);

        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            if let Some(parts_suffix) = trimmed.strip_suffix(':') {
                let parts: Vec<&str> = parts_suffix.split_whitespace().collect();
                if parts.len() >= 2 {
                    let name = parts[0].to_string();
                    let version = parts[1].trim_start_matches('v');

                    let install_date = cargo_home.as_ref().and_then(|home| {
                        let path = Path::new(home).join(".crates.toml");
                        let metadata = std::fs::metadata(&path).ok()?;
                        let modified = metadata.modified().ok()?;
                        let duration = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
                        Some(duration.as_secs() as i64)
                    });

                    packages.push(Package {
                        name: name.clone(),
                        version: version.to_string(),
                        source: PackageSource::Cargo,
                        install_date,
                        install_reason: None,
                        is_aur: false,
                        is_omarchy: false,
                        url: Some(format!("https://crates.io/crates/{}", name)),
                        size: None,
                    });
                }
            }
        }

        packages
    }
}

/// Used when CARGO_HOME isn't set — cargo's default install root is `$HOME/.cargo`.
fn dirs_fallback() -> Option<String> {
    let home = std::env::var("HOME").ok()?;
    Some(format!("{}/.cargo", home))
}
