//! Npm collector. Limited to globally-installed packages (`npm list -g
//! --depth=0 --json`) — per-project node_modules are out of scope for this
//! tool. The JSON shape we parse is `{ "dependencies": { name: { version } } }`.

use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::process::Command;

/// Globally-installed npm packages, queried via `npm list -g --depth=0 --json`.
/// npm doesn't expose install dates or sizes for global installs, so those fields stay None.
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

        let Some(deps) = parsed.get("dependencies").and_then(|d| d.as_object()) else {
            return Vec::new();
        };

        deps.iter()
            .map(|(name, info)| {
                let version = info
                    .get("version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                Package {
                    name: name.clone(),
                    version,
                    source: PackageSource::Npm,
                    install_date: None,
                    install_reason: None,
                    is_aur: false,
                    is_omarchy: false,
                    // The package name already encodes any "@scope/" prefix,
                    // so a single format covers both scoped and unscoped names.
                    url: Some(format!("https://www.npmjs.com/package/{}", name)),
                    size: None,
                }
            })
            .collect()
    }
}
