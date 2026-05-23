//! Application state and the business logic that mutates it.
//!
//! `App` is the single owner of every list, filter, and mode flag. The event
//! loop in `main.rs` reads keystrokes and calls methods on `App`; `ui.rs`
//! reads `App` immutably and renders it. Nothing in this module touches
//! ratatui or stdout - that separation keeps rendering decisions away from
//! filter/sort math.
//!
//! Notable cross-cutting design choices:
//! - `source_filter` and `omarchy_filter` are independent controls (Tab and
//!   `o` respectively). They `&&` together in `update_filtered`.
//! - Sorting is performed on the *filtered* index list, not the underlying
//!   packages - this keeps the source-of-truth stable and lets cycling
//!   filters/sorts be cheap.
//! - `details` is an `Option<PackageDetails>` populated lazily by the
//!   `details` module when the user opens the `d` panel.

use crate::collectors::Collector;
use crate::details::PackageDetails;
use crate::package::{Package, PackageSource};
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;

/// Which column drives the order of the visible table.
///
/// Version is intentionally not sortable - version strings are too noisy
/// across pacman/cargo/npm/pip for a meaningful comparison to be worth the UI
/// real estate. The remaining columns are cycled together with direction by
/// `App::cycle_sort` (the `s` key): each press advances one slot in the
/// sequence below.
#[derive(PartialEq, Eq, Clone, Copy)]
pub enum SortColumn {
    Name,
    Source,
    Size,
    InstallDate,
}

impl SortColumn {
    /// Rotation order used by `App::cycle_sort` when flipping past descending.
    pub fn next(&self) -> Self {
        match self {
            Self::Name => Self::Source,
            Self::Source => Self::Size,
            Self::Size => Self::InstallDate,
            Self::InstallDate => Self::Name,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Source => "Source",
            Self::Size => "Size",
            Self::InstallDate => "Installed",
        }
    }
}

/// Which package manager(s) to include in the visible list. Tab cycles through:
/// All → Pacman (non-Omarchy) → Omarchy → Cargo → Npm → Pip → All.
///
/// Omarchy is a virtual source: its packages are technically `PackageSource::Pacman`
/// with the `is_omarchy` flag set. We split it out here so the Tab cycle treats it
/// as a first-class filter - matching how the user thinks about it.
pub enum SourceFilter {
    All,
    Pacman,
    Omarchy,
    Specific(PackageSource),
}

impl SourceFilter {
    pub fn next(&self) -> Self {
        match self {
            Self::All => Self::Pacman,
            Self::Pacman => Self::Omarchy,
            Self::Omarchy => Self::Specific(PackageSource::Cargo),
            Self::Specific(PackageSource::Cargo) => Self::Specific(PackageSource::Npm),
            Self::Specific(PackageSource::Npm) => Self::Specific(PackageSource::Pip),
            Self::Specific(PackageSource::Pip) => Self::All,
            // Pacman as a Specific value shouldn't appear, but if it does, fold it
            // back into the canonical Self::Pacman branch.
            Self::Specific(PackageSource::Pacman) => Self::Omarchy,
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::All => "All".to_string(),
            Self::Pacman => "pacman".to_string(),
            Self::Omarchy => "omarchy".to_string(),
            Self::Specific(s) => s.label().to_string(),
        }
    }
}

/// Drives how keystrokes are interpreted by the event loop in main.rs.
///
/// `Details` is a read-only modal overlay populated by `details::fetch`
/// on demand; any keystroke (except none) dismisses it.
#[derive(PartialEq)]
pub enum InputMode {
    Normal,
    Search,
    UninstallConfirm,
    Details,
}

/// Filter for packages declared in the Omarchy base/other lists.
/// Off = show all, Only = show only Omarchy packages, Exclude = hide them.
pub enum OmarchyFilter {
    Off,
    Only,
    Exclude,
}

impl OmarchyFilter {
    pub fn next(&self) -> Self {
        match self {
            Self::Off => Self::Only,
            Self::Only => Self::Exclude,
            Self::Exclude => Self::Off,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Off => "",
            Self::Only => "[Omarchy]",
            Self::Exclude => "[−Omarchy]",
        }
    }
}

pub struct App {
    pub packages: Vec<Package>,
    pub filtered_indices: Vec<usize>,
    pub search_query: String,
    pub source_filter: SourceFilter,
    pub show_explicit_only: bool,
    pub omarchy_filter: OmarchyFilter,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub selected_index: usize,
    pub scroll_offset: u16,
    pub input_mode: InputMode,
    pub status_message: String,
    pub loading: bool,
    pub loading_progress: String,
    pub source_counts: HashMap<PackageSource, usize>,
    /// Subset of the pacman count that is also tagged as an Omarchy package.
    /// Tracked separately so the status bar can show Omarchy as its own line
    /// without inflating the pacman count.
    pub omarchy_count: usize,
    /// Lazily populated when the user opens the details panel (`d` key).
    /// Cleared when the panel is dismissed. None means "panel not open".
    pub details: Option<PackageDetails>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            packages: Vec::new(),
            filtered_indices: Vec::new(),
            search_query: String::new(),
            source_filter: SourceFilter::All,
            show_explicit_only: false,
            omarchy_filter: OmarchyFilter::Off,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            selected_index: 0,
            scroll_offset: 0,
            input_mode: InputMode::Normal,
            status_message: String::new(),
            loading: true,
            loading_progress: String::new(),
            source_counts: HashMap::new(),
            omarchy_count: 0,
            details: None,
        }
    }
}

impl App {
    /// Refresh the package list by running every enabled collector.
    ///
    /// Each collector shells out to its package manager, which is the slow part
    /// (pacman+expac in particular). They're independent, so we run one per
    /// thread and gather results through a channel - overall load time becomes
    /// the slowest collector instead of their sum.
    pub fn load(&mut self) {
        let collectors: Vec<Box<dyn Collector + Send>> = vec![
            Box::new(crate::collectors::pacman::PacmanCollector),
            Box::new(crate::collectors::cargo::CargoCollector),
            Box::new(crate::collectors::npm::NpmCollector),
            Box::new(crate::collectors::pip::PipCollector),
        ];

        let enabled: Vec<_> = collectors.into_iter().filter(|c| c.enabled()).collect();
        let total = enabled.len();
        if total == 0 {
            self.loading = false;
            self.status_message = "No supported package managers found".to_string();
            return;
        }

        let (tx, rx) = mpsc::channel();
        let mut handles = Vec::with_capacity(total);
        for collector in enabled {
            let tx = tx.clone();
            handles.push(thread::spawn(move || {
                let _ = tx.send(collector.collect());
            }));
        }
        // Drop the original sender so `rx` closes once every worker is done.
        drop(tx);

        let mut all_packages = Vec::new();
        while let Ok(batch) = rx.recv() {
            all_packages.extend(batch);
        }
        for h in handles {
            let _ = h.join();
        }

        all_packages.sort_by_key(|pkg| pkg.name.to_lowercase());

        self.source_counts.clear();
        self.omarchy_count = 0;
        for pkg in &all_packages {
            *self.source_counts.entry(pkg.source).or_insert(0) += 1;
            if pkg.is_omarchy {
                self.omarchy_count += 1;
            }
        }

        self.packages = all_packages;
        self.loading = false;
        self.loading_progress.clear();
        self.update_filtered();
        self.status_message = format!(
            "Loaded {} packages from {} sources",
            self.packages.len(),
            self.source_counts.len()
        );
    }

    pub fn update_filtered(&mut self) {
        self.filtered_indices = self
            .packages
            .iter()
            .enumerate()
            .filter(|(_, pkg)| {
                let source_match = match &self.source_filter {
                    SourceFilter::All => true,
                    // Pacman slot deliberately excludes Omarchy - Omarchy has its own slot.
                    SourceFilter::Pacman => pkg.source == PackageSource::Pacman && !pkg.is_omarchy,
                    SourceFilter::Omarchy => pkg.is_omarchy,
                    SourceFilter::Specific(s) => pkg.source == *s,
                };

                let search_match = if self.search_query.is_empty() {
                    true
                } else {
                    let query = self.search_query.to_lowercase();
                    pkg.name.to_lowercase().contains(&query)
                        || pkg.version.to_lowercase().contains(&query)
                        || pkg.source.label().contains(&query)
                };

                let explicit_match = !self.show_explicit_only
                    || pkg.install_reason.as_deref() == Some("explicit");

                let omarchy_match = match self.omarchy_filter {
                    OmarchyFilter::Off => true,
                    OmarchyFilter::Only => pkg.is_omarchy,
                    OmarchyFilter::Exclude => !pkg.is_omarchy,
                };

                source_match && search_match && explicit_match && omarchy_match
            })
            .map(|(i, _)| i)
            .collect();

        self.sort_filtered();

        if self.filtered_indices.is_empty() {
            self.selected_index = 0;
            self.scroll_offset = 0;
        } else if self.selected_index >= self.filtered_indices.len() {
            self.selected_index = self.filtered_indices.len() - 1;
        }
    }

    fn sort_filtered(&mut self) {
        let asc = self.sort_ascending;
        match self.sort_column {
            SortColumn::Name => {
                self.filtered_indices.sort_by(|a, b| {
                    let cmp = self.packages[*a]
                        .name
                        .to_lowercase()
                        .cmp(&self.packages[*b].name.to_lowercase());
                    if asc { cmp } else { cmp.reverse() }
                });
            }
            SortColumn::Size => {
                // Option<f64> has no Ord (NaN), so compare via partial_cmp and
                // treat missing sizes as "smaller than anything known" - that
                // puts unknowns at the top in ascending and at the bottom in
                // descending, which matches the natural "small → big" reading.
                self.filtered_indices.sort_by(|a, b| {
                    let sa = self.packages[*a].size;
                    let sb = self.packages[*b].size;
                    let cmp = match (sa, sb) {
                        (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal),
                        (None, Some(_)) => std::cmp::Ordering::Less,
                        (Some(_), None) => std::cmp::Ordering::Greater,
                        (None, None) => std::cmp::Ordering::Equal,
                    };
                    if asc { cmp } else { cmp.reverse() }
                });
            }
            SortColumn::Source => {
                self.filtered_indices.sort_by(|a, b| {
                    let sa = self.packages[*a].source.label();
                    let sb = self.packages[*b].source.label();
                    let cmp = sa.cmp(sb);
                    if asc { cmp } else { cmp.reverse() }
                });
            }
            SortColumn::InstallDate => {
                self.filtered_indices.sort_by(|a, b| {
                    let da = self.packages[*a].install_date;
                    let db = self.packages[*b].install_date;
                    let cmp = da.cmp(&db);
                    if asc { cmp } else { cmp.reverse() }
                });
            }
        }
    }

    /// Advance the sort by one step. Each press of `s` walks through the
    /// sequence: Name↑ → Name↓ → Source↑ → Source↓ → Size↑ → Size↓ →
    /// Installed↑ → Installed↓ → Name↑ → …  so the user can reach any
    /// (column, direction) combination with a single key.
    pub fn cycle_sort(&mut self) {
        if self.sort_ascending {
            // Same column, flip to descending.
            self.sort_ascending = false;
        } else {
            // Advance to the next column and reset to ascending.
            self.sort_column = self.sort_column.next();
            self.sort_ascending = true;
        }
        self.sort_filtered();
    }

    pub fn cycle_source_filter(&mut self) {
        self.source_filter = self.source_filter.next();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.update_filtered();
    }

    pub fn toggle_explicit(&mut self) {
        self.show_explicit_only = !self.show_explicit_only;
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.update_filtered();
    }

    pub fn toggle_omarchy(&mut self) {
        self.omarchy_filter = self.omarchy_filter.next();
        self.selected_index = 0;
        self.scroll_offset = 0;
        self.update_filtered();
    }

    pub fn open_selected_url(&self) {
        if self.filtered_indices.is_empty() || self.selected_index >= self.filtered_indices.len() {
            return;
        }
        let pkg_idx = self.filtered_indices[self.selected_index];
        if let Some(ref url) = self.packages[pkg_idx].url {
            let _ = std::process::Command::new("xdg-open")
                .arg(url)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn();
        }
    }

    pub fn selected_package(&self) -> Option<&Package> {
        if self.filtered_indices.is_empty() || self.selected_index >= self.filtered_indices.len() {
            None
        } else {
            let pkg_idx = self.filtered_indices[self.selected_index];
            Some(&self.packages[pkg_idx])
        }
    }

    /// Populate `self.details` from the currently-selected package and switch
    /// to `InputMode::Details`. Shells out to a package-manager tool - fast
    /// for one package, but not free, so we only call this on key press.
    pub fn open_details(&mut self) {
        if let Some(pkg) = self.selected_package() {
            self.details = Some(crate::details::fetch(pkg));
            self.input_mode = InputMode::Details;
        }
    }

    /// Tear down the details panel and return to normal navigation.
    pub fn close_details(&mut self) {
        self.details = None;
        self.input_mode = InputMode::Normal;
    }

    pub fn uninstall_command(&self) -> Option<Vec<String>> {
        self.selected_package().map(|pkg| match pkg.source {
            PackageSource::Pacman => vec!["sudo".into(), "pacman".into(), "-Rns".into(), pkg.name.clone()],
            PackageSource::Npm => vec!["npm".into(), "uninstall".into(), "-g".into(), pkg.name.clone()],
            PackageSource::Pip => vec!["pip".into(), "uninstall".into(), "-y".into(), pkg.name.clone()],
            PackageSource::Cargo => vec!["cargo".into(), "uninstall".into(), pkg.name.clone()],
        })
    }

    pub fn select_next(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.selected_index = (self.selected_index + 1).min(self.filtered_indices.len() - 1);
        }
    }

    pub fn select_prev(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn page_down(&mut self, page_size: usize) {
        if !self.filtered_indices.is_empty() {
            let page = page_size.max(1);
            self.selected_index =
                (self.selected_index + page).min(self.filtered_indices.len() - 1);
        }
    }

    pub fn page_up(&mut self, page_size: usize) {
        let page = page_size.max(1);
        self.selected_index = self.selected_index.saturating_sub(page);
    }

    pub fn select_first(&mut self) {
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    pub fn select_last(&mut self, list_height: usize) {
        if !self.filtered_indices.is_empty() {
            self.selected_index = self.filtered_indices.len() - 1;
            if self.filtered_indices.len() > list_height {
                self.scroll_offset = (self.filtered_indices.len() - list_height) as u16;
            }
        }
    }

    pub fn scroll_to_selection(&mut self, list_height: usize) {
        if list_height == 0 {
            return;
        }
        let list_height = list_height.min(self.filtered_indices.len());
        let sel = self.selected_index as u16;

        if sel < self.scroll_offset {
            self.scroll_offset = sel;
        } else if sel >= self.scroll_offset + list_height as u16 {
            self.scroll_offset = sel - list_height as u16 + 1;
        }

        if self.filtered_indices.len() <= list_height {
            self.scroll_offset = 0;
        }
    }
}

