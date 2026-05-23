//! The `Package` row and its source-of-truth enum.
//!
//! Every collector produces `Vec<Package>` in this shape so the UI doesn't
//! have to special-case sources at render time. Fields the source can't
//! provide stay as `None` / `false`; see the comments on each field for which
//! sources populate what.

use serde::{Deserialize, Serialize};

/// A package as displayed in the table. Fields that a particular package
/// manager can't supply (npm install dates, cargo sizes, etc.) stay None.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
    /// Unix timestamp of install, when the collector can determine it.
    pub install_date: Option<i64>,
    /// For pacman: "explicit" or "dependency" - drives the `e` filter.
    pub install_reason: Option<String>,
    /// Pacman-only: was this installed from the AUR (foreign repo)?
    pub is_aur: bool,
    /// Pacman-only: is this listed in the Omarchy package manifest?
    pub is_omarchy: bool,
    pub url: Option<String>,
    /// Disk footprint in GiB. Currently only pacman reports this.
    pub size: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PackageSource {
    Pacman,
    Cargo,
    Npm,
    Pip,
}

impl PackageSource {
    pub fn all() -> &'static [PackageSource] {
        &[Self::Pacman, Self::Cargo, Self::Npm, Self::Pip]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Pacman => "pacman",
            Self::Cargo => "cargo",
            Self::Npm => "npm",
            Self::Pip => "pip",
        }
    }
}

impl std::fmt::Display for PackageSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}
