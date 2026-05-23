use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::process::Command;

pub struct PipCollector;

impl Collector for PipCollector {
    fn enabled(&self) -> bool {
        Command::new("pip").arg("--version").output().is_ok()
    }

    fn collect(&self) -> Vec<Package> {
        let output = match Command::new("pip")
            .args(["list", "--format=json"])
            .output()
        {
            Ok(o) => o,
            Err(_) => return Vec::new(),
        };

        let stdout = String::from_utf8_lossy(&output.stdout);
        let parsed: Vec<serde_json::Value> = match serde_json::from_str(&stdout) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };

        parsed
            .iter()
            .filter_map(|pkg| {
                let name = pkg.get("name")?.as_str()?.to_string();
                let version = pkg
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                Some(Package {
                    name: name.clone(),
                    version,
                    source: PackageSource::Pip,
                    install_date: None,
                    install_reason: None,
                    is_aur: false,
                    url: Some(format!("https://pypi.org/project/{}/", name)),
                    size: None,
                })
            })
            .collect()
    }
}
