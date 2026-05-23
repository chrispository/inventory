use crate::collectors::Collector;
use crate::package::{Package, PackageSource};
use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;


pub enum SortColumn {
    Name,
    Version,
    Source,
    InstallDate,
}

impl SortColumn {
    pub fn next(&self) -> Self {
        match self {
            Self::Name => Self::Version,
            Self::Version => Self::Source,
            Self::Source => Self::InstallDate,
            Self::InstallDate => Self::Name,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Version => "Version",
            Self::Source => "Source",
            Self::InstallDate => "Installed",
        }
    }
}

pub enum SourceFilter {
    All,
    Specific(PackageSource),
}

impl SourceFilter {
    pub fn next(&self) -> Self {
        let sources = PackageSource::all();
        match self {
            Self::All => Self::Specific(sources[0]),
            Self::Specific(current) => {
                let idx = sources.iter().position(|s| s == current).unwrap_or(0);
                if idx + 1 < sources.len() {
                    Self::Specific(sources[idx + 1])
                } else {
                    Self::All
                }
            }
        }
    }

    pub fn label(&self) -> String {
        match self {
            Self::All => "All".to_string(),
            Self::Specific(s) => s.label().to_string(),
        }
    }
}

pub enum InputMode {
    Normal,
    Search,
}

pub struct App {
    pub packages: Vec<Package>,
    pub filtered_indices: Vec<usize>,
    pub search_query: String,
    pub source_filter: SourceFilter,
    pub show_explicit_only: bool,
    pub sort_column: SortColumn,
    pub sort_ascending: bool,
    pub selected_index: usize,
    pub scroll_offset: u16,
    pub input_mode: InputMode,
    pub status_message: String,
    pub loading: bool,
    pub loading_progress: String,
    pub source_counts: HashMap<PackageSource, usize>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            packages: Vec::new(),
            filtered_indices: Vec::new(),
            search_query: String::new(),
            source_filter: SourceFilter::All,
            show_explicit_only: false,
            sort_column: SortColumn::Name,
            sort_ascending: true,
            selected_index: 0,
            scroll_offset: 0,
            input_mode: InputMode::Normal,
            status_message: String::new(),
            loading: true,
            loading_progress: String::new(),
            source_counts: HashMap::new(),
        }
    }
}

impl App {
    pub fn load(&mut self) {
        let collectors: Vec<Box<dyn Collector + Send>> = vec![
            Box::new(crate::collectors::pacman::PacmanCollector),
            Box::new(crate::collectors::cargo::CargoCollector),
            Box::new(crate::collectors::npm::NpmCollector),
            Box::new(crate::collectors::pip::PipCollector),
        ];

        let (tx, rx) = mpsc::channel();
        let collector_list: Vec<_> = collectors
            .into_iter()
            .filter(|c| c.enabled())
            .collect();

        let total = collector_list.len();
        if total == 0 {
            self.loading = false;
            self.status_message = "No supported package managers found".to_string();
            return;
        }

        let mut all_packages = Vec::new();
        let mut completed = 0;

        let handle = thread::spawn(move || {
            for collector in collector_list {
                let p = collector.collect();
                let _ = tx.send(p);
            }
        });

        while let Ok(batch) = rx.recv() {
            completed += 1;
            all_packages.extend(batch);
            self.loading_progress = format!("Collected {}/{} sources...", completed, total);

            if completed >= total {
                break;
            }
        }

        let _ = handle.join();

        all_packages.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

        self.source_counts.clear();
        for pkg in &all_packages {
            *self.source_counts.entry(pkg.source).or_insert(0) += 1;
        }

        self.packages = all_packages;
        self.loading = false;
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

                source_match && search_match && explicit_match
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
            SortColumn::Version => {
                self.filtered_indices.sort_by(|a, b| {
                    let va = &self.packages[*a].version;
                    let vb = &self.packages[*b].version;
                    let cmp = compare_versions(va, vb);
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

    pub fn toggle_sort(&mut self) {
        self.sort_ascending = !self.sort_ascending;
        self.sort_filtered();
    }

    pub fn cycle_sort_column(&mut self) {
        self.sort_column = self.sort_column.next();
        self.sort_ascending = true;
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

    pub fn open_selected_url(&self) {
        if self.filtered_indices.is_empty() || self.selected_index >= self.filtered_indices.len() {
            return;
        }
        let pkg_idx = self.filtered_indices[self.selected_index];
        if let Some(ref url) = self.packages[pkg_idx].url {
            let _ = std::process::Command::new("xdg-open")
                .arg(url)
                .spawn();
        }
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

struct VersionPart {
    nums: Vec<u64>,
    suffix: String,
}

impl VersionPart {
    fn parse(s: &str) -> Self {
        let s = s.trim();
        let mut nums = Vec::new();
        let mut suffix = String::new();
        let mut current_num = String::new();
        let mut in_suffix = false;

        for ch in s.chars() {
            if ch.is_ascii_digit() && !in_suffix {
                current_num.push(ch);
            } else if ch == '.' && !in_suffix {
                if !current_num.is_empty() {
                    nums.push(current_num.parse().unwrap_or(0));
                    current_num.clear();
                }
            } else {
                in_suffix = true;
                suffix.push(ch);
            }
        }

        if !current_num.is_empty() {
            nums.push(current_num.parse().unwrap_or(0));
        }

        Self { nums, suffix }
    }
}

fn compare_versions(a: &str, b: &str) -> std::cmp::Ordering {
    let va = VersionPart::parse(a);
    let vb = VersionPart::parse(b);

    let max_len = va.nums.len().max(vb.nums.len());
    for i in 0..max_len {
        let na = va.nums.get(i).copied().unwrap_or(0);
        let nb = vb.nums.get(i).copied().unwrap_or(0);
        match na.cmp(&nb) {
            std::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }

    va.suffix.cmp(&vb.suffix)
}
