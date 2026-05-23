use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::collections::HashSet;
use std::process::Command;

pub struct PacmanCollector;

impl Collector for PacmanCollector {
    fn enabled(&self) -> bool {
        Command::new("expac").arg("-V").output().is_ok()
            && Command::new("pacman").arg("-V").output().is_ok()
    }

    fn collect(&self) -> Vec<Package> {
        let aur_packages = get_foreign_packages();

        let output = match Command::new("expac")
            .args(["-Q", "--timefmt=%s", "%n\t%v\t%l\t%w"])
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

            let mut parts = line.splitn(4, '\t');
            let name = parts.next().unwrap_or("").to_string();
            let version = parts.next().unwrap_or("").to_string();
            let date_str = parts.next().unwrap_or("");
            let reason = parts.next().unwrap_or("");

            if name.is_empty() {
                continue;
            }

            let install_date = date_str.parse::<i64>().ok();
            let is_aur = aur_packages.contains(&name);

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
                url,
                size: None,
            });
        }

        packages
    }
}

fn get_foreign_packages() -> HashSet<String> {
    match Command::new("pacman").args(["-Qmq"]).output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            stdout.lines().map(|l| l.trim().to_string()).collect()
        }
        Err(_) => HashSet::new(),
    }
}

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
