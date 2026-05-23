use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::process::Command;

pub struct NpmCollector;

impl Collector for NpmCollector {
    fn enabled(&self) -> bool {
        Command::new("npm").arg("-v").output().is_ok()
    }

    fn collect(&self) -> Vec<Package> {
        let output = match Command::new("npm")
            .args(["list", "-g", "--depth=0", "--json"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        let mut packages = Vec::new();

        if let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_object()) {
            for (name, info) in deps {
                let version = info
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let url = if name.starts_with('@') {
                    let (scope, rest) = name
                        .split_once('/')
                        .map(|(s, r)| (s, r))
                        .unwrap_or((&name, ""));
                    Some(format!("https://www.npmjs.com/package/@{}/{}", scope, rest))
                } else {
                    Some(format!("https://www.npmjs.com/package/{}", name))
                };

                packages.push(Package {
                    name: name.clone(),
                    version,
                    source: PackageSource::Npm,
                    install_date: None,
                    install_reason: None,
                    is_aur: false,
                    url,
                    size: None,
                });
            }
        }

        packages
    }
}
