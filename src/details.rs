//! Per-package "details" panel data and lazy fetchers.
//!
//! When the user presses `d` on a row, `fetch` is called for just that one
//! package. We never collect this info during the bulk load — it's only
//! shelled out for on demand because pacman -Qi/pip show are slow when run
//! per-package across hundreds of rows.
//!
//! Each backing tool returns a different text format. The fillers below
//! normalise everything into the same `PackageDetails` shape so the renderer
//! in `ui.rs` doesn't have to branch on `PackageSource`. Fields that a given
//! source can't supply (e.g. npm has no install date for globals) stay None /
//! empty and the renderer just skips that section.

use crate::package::{Package, PackageSource};
use std::process::Command;

/// Enable a reverse-dependency scan for npm globals.
///
/// When true, opening the details panel for an npm package walks every
/// globally-installed package.json under `$(npm root -g)` and collects those
/// whose `dependencies` map references the target package. Correct, but adds
/// noticeable latency (one disk read per global) — disabled by default.
/// Flip to `true` here, or comment out the call site in `fill_npm`, to toggle.
const NPM_REVERSE_DEPS: bool = false;

/// Read-only snapshot of everything we can dig up about a single package.
///
/// Optional scalar fields stay None when the source doesn't supply them;
/// list fields stay empty. `notes` holds free-form messages for cases where
/// a source genuinely cannot answer (e.g. cargo doesn't persist dep info for
/// installed binaries) so the renderer can surface that to the user instead
/// of showing a blank section.
pub struct PackageDetails {
    pub name: String,
    pub version: String,
    pub source: PackageSource,

    pub description: Option<String>,
    pub license: Option<String>,
    /// Repo name (pacman: core/extra/multilib/AUR) or registry origin.
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub installed_size: Option<String>,
    pub install_date: Option<String>,
    pub build_date: Option<String>,
    pub install_reason: Option<String>,

    pub depends_on: Vec<String>,
    /// Each entry may be `"name"` or `"name: short reason"` (pacman style).
    pub optional_deps: Vec<String>,
    pub required_by: Vec<String>,
    pub optional_for: Vec<String>,

    /// Free-form messages shown verbatim at the bottom of the panel
    /// (e.g. "cargo doesn't track dependencies for installed binaries").
    pub notes: Vec<String>,
}

impl PackageDetails {
    fn new(pkg: &Package) -> Self {
        Self {
            name: pkg.name.clone(),
            version: pkg.version.clone(),
            source: pkg.source,
            description: None,
            license: None,
            repository: None,
            homepage: pkg.url.clone(),
            installed_size: None,
            install_date: None,
            build_date: None,
            install_reason: pkg.install_reason.clone(),
            depends_on: Vec::new(),
            optional_deps: Vec::new(),
            required_by: Vec::new(),
            optional_for: Vec::new(),
            notes: Vec::new(),
        }
    }
}

/// Look up details for a single package by shelling out to the appropriate
/// tool. Always returns a `PackageDetails`; on failure, fields are left empty
/// and a note is appended so the popup is never blank.
pub fn fetch(pkg: &Package) -> PackageDetails {
    let mut d = PackageDetails::new(pkg);
    match pkg.source {
        PackageSource::Pacman => fill_pacman(&mut d),
        PackageSource::Pip => fill_pip(&mut d),
        PackageSource::Npm => fill_npm(&mut d),
        PackageSource::Cargo => fill_cargo(&mut d),
    }
    d
}

// ---------------------------------------------------------------------------
// Pacman
// ---------------------------------------------------------------------------

/// Parse `pacman -Qi <name>`. The output is a sequence of `Key : Value` lines
/// (the colon is preceded by spaces that pad the key column). Multi-value
/// fields like `Depends On` and `Optional Deps` are space- or newline-separated
/// with continuation lines indented by whitespace.
fn fill_pacman(d: &mut PackageDetails) {
    let output = match Command::new("pacman").args(["-Qi", &d.name]).output() {
        Ok(o) if o.status.success() => o,
        _ => {
            d.notes.push("pacman -Qi failed for this package".to_string());
            return;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    for (key, value) in parse_colon_records(&stdout) {
        match key.as_str() {
            "Description" => d.description = none_if_empty(&value),
            "Licenses" => d.license = none_if_empty(&value),
            "Repository" => d.repository = none_if_empty(&value),
            // Prefer the pre-populated homepage from the Package row, falling
            // back to the value pacman reports when we don't already have one.
            "URL" if d.homepage.is_none() => d.homepage = none_if_empty(&value),
            "Installed Size" => d.installed_size = none_if_empty(&value),
            "Install Date" => d.install_date = none_if_empty(&value),
            "Build Date" => d.build_date = none_if_empty(&value),
            "Install Reason" => d.install_reason = none_if_empty(&value),
            "Depends On" => d.depends_on = split_pacman_list(&value),
            "Optional Deps" => d.optional_deps = split_pacman_optional(&value),
            "Required By" => d.required_by = split_pacman_list(&value),
            "Optional For" => d.optional_for = split_pacman_list(&value),
            _ => {}
        }
    }
}

/// Pacman uses two spaces between items on a single line for `Depends On` /
/// `Required By` / `Optional For`, with `None` as the sentinel for empty.
fn split_pacman_list(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "None" {
        return Vec::new();
    }
    trimmed
        .split_whitespace()
        .map(|s| s.to_string())
        .collect()
}

/// `Optional Deps` is one per line, formatted as `name: reason [installed]`.
/// We keep the colon-reason part because that's what makes it useful.
fn split_pacman_optional(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "None" {
        return Vec::new();
    }
    trimmed
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------
// Pip
// ---------------------------------------------------------------------------

/// Parse `pip show <name>`. Output is RFC822-ish: `Key: Value` per line, with
/// continuation lines beginning with whitespace. `Requires` and `Required-by`
/// are comma-separated package names.
fn fill_pip(d: &mut PackageDetails) {
    let output = match Command::new("pip").args(["show", &d.name]).output() {
        Ok(o) if o.status.success() => o,
        _ => {
            d.notes.push("pip show failed for this package".to_string());
            return;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    for (key, value) in parse_colon_records(&stdout) {
        match key.as_str() {
            "Summary" => d.description = none_if_empty(&value),
            "License" => d.license = none_if_empty(&value),
            // Pip emits an empty `Home-page:` line when the package didn't set
            // one — treat that as "no homepage" and fall back to pacman-style
            // override-only-if-unset behaviour.
            "Home-page"
                if d.homepage.is_none() || d.homepage.as_deref() == Some("") =>
            {
                d.homepage = none_if_empty(&value);
            }
            "Location" => d.repository = none_if_empty(&value),
            "Requires" => d.depends_on = split_csv(&value),
            "Required-by" => d.required_by = split_csv(&value),
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Npm
// ---------------------------------------------------------------------------

/// Read the globally-installed package's `package.json` directly. npm CLI calls
/// like `npm view` would hit the registry, but we want strictly local info.
fn fill_npm(d: &mut PackageDetails) {
    let root = match Command::new("npm").args(["root", "-g"]).output() {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().to_string()
        }
        _ => {
            d.notes.push("could not resolve `npm root -g`".to_string());
            return;
        }
    };

    let pkg_json = format!("{}/{}/package.json", root, d.name);
    let raw = match std::fs::read_to_string(&pkg_json) {
        Ok(s) => s,
        Err(_) => {
            d.notes
                .push(format!("package.json not found at {}", pkg_json));
            return;
        }
    };

    let parsed: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            d.notes.push("could not parse package.json".to_string());
            return;
        }
    };

    d.description = parsed
        .get("description")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    d.license = parsed
        .get("license")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    if d.homepage.is_none() || d.homepage.as_deref() == Some("") {
        d.homepage = parsed
            .get("homepage")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
    }
    if let Some(deps) = parsed.get("dependencies").and_then(|v| v.as_object()) {
        d.depends_on = deps
            .iter()
            .map(|(name, ver)| {
                let v = ver.as_str().unwrap_or("");
                if v.is_empty() {
                    name.clone()
                } else {
                    format!("{} {}", name, v)
                }
            })
            .collect();
    }

    if NPM_REVERSE_DEPS {
        d.required_by = npm_reverse_deps(&root, &d.name);
    } else {
        d.notes.push(
            "reverse-dep scan disabled for npm (toggle NPM_REVERSE_DEPS in details.rs)"
                .to_string(),
        );
    }
}

/// Walk every package.json under the npm global root and collect the names of
/// packages that list `target` as a direct dependency. Slow but exhaustive.
///
/// Only invoked when NPM_REVERSE_DEPS is true.
#[allow(dead_code)]
fn npm_reverse_deps(npm_root: &str, target: &str) -> Vec<String> {
    let entries = match std::fs::read_dir(npm_root) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        // Handle both flat (`/lodash/package.json`) and scoped (`/@scope/foo/package.json`) layouts.
        let scoped = path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with('@'));
        let candidates: Vec<std::path::PathBuf> = if scoped {
            std::fs::read_dir(&path)
                .ok()
                .into_iter()
                .flat_map(|it| it.flatten().map(|e| e.path().join("package.json")))
                .collect()
        } else {
            vec![path.join("package.json")]
        };

        for pkg_json in candidates {
            let Ok(raw) = std::fs::read_to_string(&pkg_json) else { continue };
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else { continue };
            let depends_on_target = parsed
                .get("dependencies")
                .and_then(|d| d.as_object())
                .is_some_and(|m| m.contains_key(target));
            if depends_on_target
                && let Some(name) = parsed.get("name").and_then(|v| v.as_str())
            {
                out.push(name.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// ---------------------------------------------------------------------------
// Cargo
// ---------------------------------------------------------------------------

/// Cargo doesn't persist dependency info for installed binaries — `cargo
/// install --list` only knows the crate name, version, and provided binaries.
/// We surface that limitation as a note so the user isn't left wondering why
/// the deps sections are empty.
fn fill_cargo(d: &mut PackageDetails) {
    d.notes.push(
        "cargo doesn't record dependency info for binaries installed via `cargo install`."
            .to_string(),
    );
    d.notes
        .push("Run `cargo info {name}` in a shell to see the published deps.".replace("{name}", &d.name));
}

// ---------------------------------------------------------------------------
// Shared parsers
// ---------------------------------------------------------------------------

/// Parse a `Key : Value` / `Key: Value` text block (used by both pacman -Qi and
/// pip show). Continuation lines that start with whitespace are appended to the
/// previous key's value separated by a newline so multi-line fields stay
/// distinguishable downstream.
fn parse_colon_records(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        // Continuation: starts with whitespace and we have a prior key.
        if line.starts_with(char::is_whitespace)
            && let Some(last) = out.last_mut()
        {
            last.1.push('\n');
            last.1.push_str(line.trim());
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            out.push((k.trim().to_string(), v.trim().to_string()));
        }
    }
    out
}

fn split_csv(value: &str) -> Vec<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    trimmed
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn none_if_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() || t == "None" {
        None
    } else {
        Some(t.to_string())
    }
}
