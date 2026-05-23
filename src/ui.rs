//! All ratatui rendering lives here. Functions in this module take `&App`
//! and never mutate it - every state change goes through `App` methods called
//! from the event loop in `main.rs`.
//!
//! Layout overview (top to bottom):
//! - row 0: header bar (source/search/sort summary)
//! - row 1: help hint (changes by `InputMode`)
//! - middle: the package table (with ▶ selector and per-column sort arrows)
//! - last row: status bar (counts + last status message)
//!
//! Modal overlays (uninstall confirm, details panel) are drawn after the
//! base layout so they sit on top.

use crate::app::{App, InputMode, SortColumn, SourceFilter};
use crate::details::PackageDetails;
use crate::package::PackageSource;
use chrono::TimeZone;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Cell, HighlightSpacing, Paragraph, Row, Scrollbar,
        ScrollbarOrientation, ScrollbarState, Table, TableState, Wrap,
    },
    Frame,
};

const HEADER_STYLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const SELECTED_STYLE: Style = Style::new().bg(Color::DarkGray);
const PACMAN_COLOR: Color = Color::Green;
const CARGO_COLOR: Color = Color::Yellow;
const NPM_COLOR: Color = Color::Red;
const PIP_COLOR: Color = Color::Blue;
const AUR_COLOR: Color = Color::Magenta;
const OMARCHY_COLOR: Color = Color::Cyan;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(area);

    draw_header(f, layout[0], app);
    draw_help(f, layout[1], app);

    if app.loading {
        draw_loading(f, layout[2], app);
    } else {
        draw_table(f, layout[2], app);
    }

    draw_status(f, layout[3], app);

    if app.input_mode == InputMode::UninstallConfirm {
        draw_uninstall_popup(f, area, app);
    }
    // Details overlay sits on top of everything else - including the
    // uninstall popup, though they're mutually exclusive in practice.
    if let Some(details) = app.details.as_ref() {
        draw_details_popup(f, area, details);
    }
}

/// Modal confirmation rendered on top of the table when the user presses `X`.
/// Width is the longest content line plus generous side padding, with a floor
/// so short package names still produce a comfortably-sized dialog that matches
/// the table's bordered/titled style.
fn draw_uninstall_popup(f: &mut Frame, area: Rect, app: &App) {
    let Some(pkg) = app.selected_package() else {
        return;
    };

    let bold_white = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
    let dim = Style::new().fg(Color::Gray);

    let lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("Uninstall {}?", pkg.name), bold_white),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("{} v{}", pkg.name, pkg.version), Style::new().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled(format!("from {}", pkg.source.label()), dim),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("[y]", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::styled(" confirm    ", Style::new().fg(Color::White)),
            Span::styled("[any other key]", Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(" cancel", Style::new().fg(Color::White)),
        ]),
        Line::from(""),
    ];

    // Borders + 4 cells of side padding (2 baked into the text, 2 inside the border).
    let content_width = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16;
    let popup_width = (content_width + 6)
        .max(56)
        .min(area.width.saturating_sub(4));
    let popup_height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));

    let popup_area = Rect {
        x: area.x + (area.width.saturating_sub(popup_width)) / 2,
        y: area.y + (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Red))
        .title(Span::styled(
            " Uninstall ",
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines).block(block);

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

/// Render the read-only "Details" overlay populated by `details::fetch`.
///
/// The popup occupies ~80% of the screen and reserves the inner area for a
/// wrapped paragraph. Each section (description, meta, depends-on, …) is built
/// from the `PackageDetails` struct; empty sections are skipped so the panel
/// gracefully shrinks for sources that supply less data (e.g. cargo).
fn draw_details_popup(f: &mut Frame, area: Rect, details: &PackageDetails) {
    // Sizing: 80% of each dimension, with sane floors so it still looks like
    // a panel on very small terminals.
    let popup_width = ((area.width as u32 * 8 / 10) as u16).max(50).min(area.width);
    let popup_height = ((area.height as u32 * 8 / 10) as u16).max(15).min(area.height);
    let popup_area = Rect {
        x: area.x + (area.width.saturating_sub(popup_width)) / 2,
        y: area.y + (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    let lines = build_details_lines(details);

    let title = Span::styled(
        format!(" Details: {} ", details.name),
        Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Cyan))
        .title(title);

    let paragraph = Paragraph::new(lines).block(block).wrap(Wrap { trim: false });

    f.render_widget(ratatui::widgets::Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}

/// Build the body of the details popup as a list of styled `Line`s.
///
/// Section order is intentional: identity → meta → forward deps → reverse deps
/// → notes. Sections with no data are simply omitted.
fn build_details_lines<'a>(d: &'a PackageDetails) -> Vec<Line<'a>> {
    let dim = Style::new().fg(Color::Gray);
    let label = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
    let bold_white = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);

    let mut out: Vec<Line> = Vec::new();
    out.push(Line::from(""));

    // Header line: "foo 1.2.3 - pacman"
    out.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(d.name.clone(), bold_white),
        Span::raw(" "),
        Span::styled(format!("v{}", d.version), Style::new().fg(Color::White)),
        Span::raw("  "),
        Span::styled(format!("- {}", d.source.label()), dim),
    ]));

    if let Some(desc) = &d.description {
        out.push(Line::from(""));
        out.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(desc.clone(), Style::new().fg(Color::White)),
        ]));
    }

    // Meta block: build a list of "Label: value" pairs that exist.
    let mut meta: Vec<(&str, String)> = Vec::new();
    if let Some(v) = &d.license {
        meta.push(("License", v.clone()));
    }
    if let Some(v) = &d.installed_size {
        meta.push(("Size", v.clone()));
    }
    if let Some(v) = &d.repository {
        meta.push(("Repo", v.clone()));
    }
    if let Some(v) = &d.install_reason {
        meta.push(("Reason", v.clone()));
    }
    if let Some(v) = &d.install_date {
        meta.push(("Installed", v.clone()));
    }
    if let Some(v) = &d.build_date {
        meta.push(("Built", v.clone()));
    }
    if let Some(v) = &d.homepage {
        meta.push(("URL", v.clone()));
    }
    if !meta.is_empty() {
        out.push(Line::from(""));
        for (k, v) in meta {
            out.push(Line::from(vec![
                Span::raw("  "),
                Span::styled(format!("{}: ", k), label),
                Span::styled(v, Style::new().fg(Color::White)),
            ]));
        }
    }

    push_list_section(&mut out, "Depends on", &d.depends_on, label);
    push_list_section(&mut out, "Optional deps", &d.optional_deps, label);
    push_list_section(&mut out, "Required by", &d.required_by, label);
    push_list_section(&mut out, "Optional for", &d.optional_for, label);

    if !d.notes.is_empty() {
        out.push(Line::from(""));
        for note in &d.notes {
            out.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("note: ", Style::new().fg(Color::Magenta)),
                Span::styled(note.clone(), dim),
            ]));
        }
    }

    out
}

/// Append a section like:
///
/// ```text
///   Depends on:
///     a, b, c, d
/// ```
///
/// to `out`. No-op when `items` is empty so the panel collapses naturally.
fn push_list_section<'a>(
    out: &mut Vec<Line<'a>>,
    title: &'a str,
    items: &[String],
    label_style: Style,
) {
    if items.is_empty() {
        return;
    }
    out.push(Line::from(""));
    out.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(format!("{}:", title), label_style),
    ]));
    // Join with commas; Paragraph::wrap handles the visual line breaks.
    out.push(Line::from(vec![
        Span::raw("    "),
        Span::styled(items.join(", "), Style::new().fg(Color::White)),
    ]));
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let mut tags = Vec::new();
    if app.show_explicit_only {
        tags.push("[Explicit]");
    }
    let om_label = app.omarchy_filter.label();
    if !om_label.is_empty() {
        tags.push(om_label);
    }
    let tag_str = if tags.is_empty() {
        String::new()
    } else {
        format!(" {}", tags.join(" "))
    };
    let left = format!(
        " Source: {}{} ({}) ",
        app.source_filter.label(),
        tag_str,
        app.packages.len()
    );

    let search = match app.input_mode {
        InputMode::Search => format!(" Search: {}│ ", app.search_query),
        // Details/UninstallConfirm/Normal all show the current query (if any)
        // in passive form - they don't accept search input.
        InputMode::Normal | InputMode::UninstallConfirm | InputMode::Details => {
            if app.search_query.is_empty() {
                " /search ".to_string()
            } else {
                format!(" Search: {} ", app.search_query)
            }
        }
    };

    let sort = format!(
        " Sort: {} {} ",
        app.sort_column.label(),
        if app.sort_ascending { "▲" } else { "▼" }
    );

    let spacing = area.width as usize;
    let total_len = left.len() + search.len() + sort.len();
    let padding = if spacing > total_len + 4 {
        spacing - total_len - 4
    } else {
        2
    };

    let text = format!(
        "{}{}{}{}{}",
        left,
        " ".repeat((padding / 2).max(1)),
        search,
        " ".repeat((padding - padding / 2).max(1)),
        sort
    );

    let paragraph = Paragraph::new(text)
        .style(Style::new().bg(Color::DarkGray).fg(Color::White))
        .block(Block::default());
    f.render_widget(paragraph, area);
}

fn draw_help(f: &mut Frame, area: Rect, app: &App) {
    match app.input_mode {
        InputMode::Search => {
            let hint = Span::styled(
                " filtering live - Enter: done  Esc: exit search ",
                Style::new().bg(Color::DarkGray).fg(Color::Yellow),
            );
            let text = Line::from(hint);
            f.render_widget(Paragraph::new(text), area);
        }
        InputMode::UninstallConfirm => {
            let hint = Span::styled(" y: confirm uninstall  any other key: cancel ", Style::new().bg(Color::DarkGray).fg(Color::Red));
            let text = Line::from(hint);
            f.render_widget(Paragraph::new(text), area);
        }
        InputMode::Details => {
            let hint = Span::styled(
                " details - any key closes ",
                Style::new().bg(Color::DarkGray).fg(Color::Cyan),
            );
            f.render_widget(Paragraph::new(Line::from(hint)), area);
        }
        InputMode::Normal => {
            let spans = vec![
                Span::styled(" j/k:nav ", Style::new().fg(Color::DarkGray)),
                Span::styled(" /:search ", Style::new().fg(Color::DarkGray)),
                Span::styled(" Tab:source ", Style::new().fg(Color::DarkGray)),
                Span::styled(" e:explicit ", Style::new().fg(Color::DarkGray)),
                Span::styled(" o:omarchy ", Style::new().fg(Color::DarkGray)),
                Span::styled(" s:sort ", Style::new().fg(Color::DarkGray)),
                Span::styled(" Ent:open ", Style::new().fg(Color::DarkGray)),
                Span::styled(" d:details ", Style::new().fg(Color::DarkGray)),
                Span::styled(" X:uninstall ", Style::new().fg(Color::DarkGray)),
                Span::styled(" q:quit ", Style::new().fg(Color::DarkGray)),
            ];
            let text = Line::from(spans);
            f.render_widget(Paragraph::new(text).style(Style::new().fg(Color::Gray)), area);
        }
    }
}

fn draw_loading(f: &mut Frame, area: Rect, app: &App) {
    let text = format!(
        "\n\n  Loading packages...\n  {}\n\n  Please wait...",
        app.loading_progress
    );
    let paragraph = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .title(" Inventory ")
                .title_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        )
        .centered();
    f.render_widget(paragraph, area);
}

fn draw_table(f: &mut Frame, area: Rect, app: &App) {
    // -3 = top border, header row, bottom border.
    let available_height = (area.height as usize).saturating_sub(3);
    if available_height == 0 {
        return;
    }

    let name_width: u16 = 30;

    let header = Row::new(vec![
        Cell::from(sort_header("Name", app, SortColumn::Name)),
        Cell::from("Site"),
        // Version is not sortable - version strings vary too much across
        // package managers to compare meaningfully.
        Cell::from("Version"),
        Cell::from(sort_header("Source", app, SortColumn::Source)),
        Cell::from(sort_header("Size", app, SortColumn::Size)),
        Cell::from(sort_header("Installed", app, SortColumn::InstallDate)),
    ])
    .style(HEADER_STYLE)
    .height(1);

    let start = app.scroll_offset as usize;
    let end = (start + available_height).min(app.filtered_indices.len());
    let visible: Vec<usize> = (start..end).collect();

    let rows: Vec<Row> = visible
        .iter()
        .map(|&idx| {
            let pkg_idx = app.filtered_indices[idx];
            let pkg = &app.packages[pkg_idx];

            let name = truncate(&pkg.name, name_width as usize - 2);
            let site = short_site(&pkg.url);
            let version = truncate(&pkg.version, 18);

            let source_color = match pkg.source {
                PackageSource::Pacman if pkg.is_omarchy => OMARCHY_COLOR,
                PackageSource::Pacman if pkg.is_aur => AUR_COLOR,
                PackageSource::Pacman => PACMAN_COLOR,
                PackageSource::Cargo => CARGO_COLOR,
                PackageSource::Npm => NPM_COLOR,
                PackageSource::Pip => PIP_COLOR,
            };

            let source_label = if pkg.is_omarchy {
                "om".to_string()
            } else if pkg.is_aur {
                "AUR".to_string()
            } else {
                pkg.source.label().to_string()
            };

            let size_str = pkg.size.map(|gb| format!("{:.2} GB", gb)).unwrap_or_else(|| "-".to_string());

            let date_str = pkg
                .install_date
                .and_then(|ts| {
                    chrono::Utc
                        .timestamp_opt(ts, 0)
                        .single()
                        .map(|dt| dt.format("%Y-%m-%d").to_string())
                })
                .unwrap_or_else(|| "-".to_string());

            Row::new(vec![
                Cell::from(name),
                Cell::from(site),
                Cell::from(version),
                Cell::from(source_label).style(Style::new().fg(source_color)),
                Cell::from(size_str),
                Cell::from(date_str),
            ])
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(name_width),
        Constraint::Length(10), // Site
        Constraint::Length(20), // Version
        Constraint::Length(10), // Source - widened from 8 so "Source ▲" has padding
        Constraint::Length(10), // Size
        Constraint::Length(12), // Installed
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(format!(
            " Inventory [{} of {}] ",
            app.filtered_indices.len(),
            app.packages.len()
        ))
        .title_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let mut table_state = TableState::new();
    if let Some(sel_idx) = visible.iter().position(|&idx| idx == app.selected_index) {
        table_state.select(Some(sel_idx));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .row_highlight_style(SELECTED_STYLE)
        .highlight_symbol("▶ ")
        // Reserve the highlight-symbol column unconditionally; otherwise rows
        // reflow leftward whenever the selected row scrolls out of view.
        .highlight_spacing(HighlightSpacing::Always);

    f.render_stateful_widget(table, area, &mut table_state);

    if app.filtered_indices.len() > available_height {
        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"));
        let mut scrollbar_state = ScrollbarState::new(app.filtered_indices.len())
            .position(app.scroll_offset as usize);
        let scrollbar_area = Rect {
            x: area.x + area.width - 1,
            y: area.y + 1,
            width: 1,
            height: area.height.saturating_sub(2),
        };
        f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
    }
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let mut parts = Vec::new();

    if let SourceFilter::All = app.source_filter {
        for source in PackageSource::all() {
            let Some(count) = app.source_counts.get(source) else { continue };
            // Pacman count is the union of regular pacman + omarchy; split them
            // here so the status bar matches the Tab cycle's separation.
            if *source == PackageSource::Pacman {
                let pure_pacman = count.saturating_sub(app.omarchy_count);
                parts.push(format!("pacman:{}", pure_pacman));
                if app.omarchy_count > 0 {
                    parts.push(format!("omarchy:{}", app.omarchy_count));
                }
            } else {
                parts.push(format!("{}:{}", source.label(), count));
            }
        }
    }

    let status = if parts.is_empty() {
        format!(
            " {} packages showing | {} ",
            app.filtered_indices.len(),
            app.status_message
        )
    } else {
        format!(
            " {} - showing {} | {} ",
            parts.join(" "),
            app.filtered_indices.len(),
            app.status_message
        )
    };

    let paragraph = Paragraph::new(status)
        .style(Style::new().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(paragraph, area);
}

/// Render a column header with an ▲/▼ arrow when this column is the active sort.
/// The arrow is rendered as a separate span so it can pick up its own colour
/// without dimming the header label.
fn sort_header<'a>(label: &'a str, app: &App, col: SortColumn) -> Line<'a> {
    if app.sort_column == col {
        let arrow = if app.sort_ascending { " ▲" } else { " ▼" };
        Line::from(vec![
            Span::raw(label),
            Span::styled(arrow, Style::new().fg(Color::Yellow)),
        ])
    } else {
        Line::from(label)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max < 3 {
        s[..max].to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

fn short_site(url: &Option<String>) -> String {
    match url {
        None => "-".to_string(),
        Some(u) => {
            if u.contains("aur.archlinux") {
                "aur".to_string()
            } else if u.contains("archlinux") {
                "archlinux".to_string()
            } else if u.contains("npmjs") {
                "npmjs".to_string()
            } else if u.contains("pypi") {
                "pypi".to_string()
            } else if u.contains("crates.io") {
                "crates.io".to_string()
            } else {
                "link".to_string()
            }
        }
    }
}
