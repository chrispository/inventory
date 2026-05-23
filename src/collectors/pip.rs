//! Pip collector. Reports whatever environment the on-PATH `pip` resolves to -
//! that means if a venv is active when inventory starts, only that venv's
//! packages will appear. `pip list --format=json` includes transitive deps,
//! so the "explicit" filter currently produces inflated results for pip rows
//! (a known limitation noted in details.rs and the README todo).

use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::process::Command;

/// Python packages from whatever `pip` resolves to on $PATH. Note that this
/// reports the currently-active environment - if pip is shimmed by pyenv or a
/// venv is active when inventory launches, only that environment's packages show.
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
                    is_omarchy: false,
                    url: Some(format!("https://pypi.org/project/{}/", name)),
                    size: None,
                })
            })
            .collect()
    }
}
