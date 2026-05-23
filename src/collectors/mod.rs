//! Per-package-manager collectors. Adding support for a new source means:
//! 1. Add a variant to `PackageSource` in `package.rs`.
//! 2. Add a submodule here implementing `Collector`.
//! 3. Register it in `App::load` in `app.rs`.
//! 4. Optionally extend `details::fetch` so the `d` panel works for it.
//!
//! Collectors are invoked in parallel from `App::load` (one thread each), so
//! every implementation must be `Send` and should not share mutable state.

use crate::package::Package;

/// One package source (pacman, cargo, npm, …). Implementations shell out to the
/// underlying tool; `enabled` lets us skip sources whose binary isn't installed.
pub trait Collector {
    /// Cheap check - typically a `--version` call - used by App::load to skip
    /// sources that aren't available on this machine.
    fn enabled(&self) -> bool;
    /// Run the (potentially slow) tool and parse its output into Packages.
    fn collect(&self) -> Vec<Package>;
}

pub mod pacman;
pub mod cargo;
pub mod npm;
pub mod pip;
