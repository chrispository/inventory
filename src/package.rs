use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
    pub source: PackageSource,
    pub install_date: Option<i64>,
    pub install_reason: Option<String>,
    pub is_aur: bool,
    pub url: Option<String>,
    pub size: Option<String>,
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
