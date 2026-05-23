//! Pacman collector. Uses `expac` (a fast, scriptable wrapper around the
//! pacman DB) for the bulk query because `pacman -Q` doesn't expose install
//! date / size / reason in a single pass. Falls back to nothing if expac
//! isn't installed - see `enabled()`.

use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// Pacman packages, queried via `expac` (a fast pacman field formatter).
/// Each line is `name\tversion\tinstall_epoch\treason\tinstalled_size_bytes`.
/// We cross-reference foreign packages (AUR) and Omarchy's manifest to tag rows.
pub struct PacmanCollector;

impl Collector for PacmanCollector {
    fn enabled(&self) -> bool {
        Command::new("expac").arg("-V").output().is_ok()
            && Command::new("pacman").arg("-V").output().is_ok()
    }

    fn collect(&self) -> Vec<Package> {
        let aur_packages = get_foreign_packages();
        let omarchy_packages = get_omarchy_packages();

        let output = match Command::new("expac")
            .args(["-Q", "--timefmt=%s", "%n\t%v\t%l\t%w\t%m"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut packages = Vec::new();

        for line in stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            let mut parts = line.splitn(5, '\t');
            let name = parts.next().unwrap_or("").to_string();
            let version = parts.next().unwrap_or("").to_string();
            let date_str = parts.next().unwrap_or("");
            let reason = parts.next().unwrap_or("");
            let size_bytes = parts.next().unwrap_or("").parse::<u64>().ok();

            if name.is_empty() {
                continue;
            }

            let install_date = date_str.parse::<i64>().ok();
            let is_aur = aur_packages.contains(&name);
            let is_omarchy = omarchy_packages.contains(&name);
            let size = size_bytes.map(|b| b as f64 / 1_073_741_824.0);

            let url = if is_aur {
                Some(format!("https://aur.archlinux.org/packages/{}", name))
            } else {
                Some(format!(
                    "https://archlinux.org/packages/?q={}",
                    urlencoding(&name)
                ))
            };

            packages.push(Package {
                name,
                version,
                source: PackageSource::Pacman,
                install_date,
                install_reason: Some(reason.to_string()),
                is_aur,
                is_omarchy,
                url,
                size,
            });
        }

        packages
    }
}

/// Names of packages installed from outside the official repos (typically AUR).
fn get_foreign_packages() -> HashSet<String> {
    match Command::new("pacman").args(["-Qmq"]).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.lines().map(|l| l.trim().to_string()).collect()
        }
        Err(_) => HashSet::new(),
    }
}

/// Packages declared in Omarchy's install manifests under ~/.local/share/omarchy/install.
/// Lines starting with `#` are comments; blank lines are ignored.
fn get_omarchy_packages() -> HashSet<String> {
    let home = match std::env::var("HOME") {
        Ok(h) => h,
        Err(_) => return HashSet::new(),
    };

    let files = [
        format!("{}/.local/share/omarchy/install/omarchy-base.packages", home),
        format!("{}/.local/share/omarchy/install/omarchy-other.packages", home),
    ];

    let mut packages = HashSet::new();
    for path in &files {
        if let Ok(content) = std::fs::read_to_string(Path::new(path)) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                packages.insert(trimmed.to_string());
            }
        }
    }

    packages
}

/// Tiny percent-encoder for the archlinux.org search URL - avoids pulling in
/// a full URL-encoding crate for one query string.
fn urlencoding(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}
