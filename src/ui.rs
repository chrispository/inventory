use crate::app::{App, InputMode, SourceFilter};
use crate::package::PackageSource;
use chrono::TimeZone;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState},
    Frame,
};

const HEADER_STYLE: Style = Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD);
const SELECTED_STYLE: Style = Style::new().bg(Color::DarkGray);
const PACMAN_COLOR: Color = Color::Green;
const CARGO_COLOR: Color = Color::Yellow;
const NPM_COLOR: Color = Color::Red;
const PIP_COLOR: Color = Color::Blue;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
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

    draw_urlbar(f, layout[3], app);
    draw_status(f, layout[4], app);
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let explicit_label = if app.show_explicit_only {
        " [Explicit]"
    } else {
        ""
    };
    let left = format!(
        " Source: {}{} ({}) ",
        app.source_filter.label(),
        explicit_label,
        app.packages.len()
    );

    let search = match app.input_mode {
        InputMode::Search => format!(" Search: {}│ ", app.search_query),
        InputMode::Normal => {
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
            let hint = Span::styled(" Enter: apply  Esc: cancel ", Style::new().bg(Color::DarkGray).fg(Color::Yellow));
            let text = Line::from(hint);
            f.render_widget(Paragraph::new(text), area);
        }
        InputMode::Normal => {
            let spans = vec![
                Span::styled(" j/k:nav ", Style::new().fg(Color::DarkGray)),
                Span::styled(" /:search ", Style::new().fg(Color::DarkGray)),
                Span::styled(" Tab:source ", Style::new().fg(Color::DarkGray)),
                Span::styled(" e:explicit ", Style::new().fg(Color::DarkGray)),
                Span::styled(" s:sort ", Style::new().fg(Color::DarkGray)),
                Span::styled(" r:reverse ", Style::new().fg(Color::DarkGray)),
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
    let available_height = (area.height as usize).saturating_sub(3);
    if available_height == 0 {
        return;
    }

    let name_width = (area.width as usize).saturating_sub(48);
    let name_width = name_width.max(10) as u16;

    let header = Row::new(vec![
        Cell::from("Name"),
        Cell::from("Version"),
        Cell::from("Source"),
        Cell::from("Installed"),
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
            let version = truncate(&pkg.version, 22);

            let source_color = match pkg.source {
                PackageSource::Pacman if pkg.is_aur => Color::Magenta,
                PackageSource::Pacman => PACMAN_COLOR,
                PackageSource::Cargo => CARGO_COLOR,
                PackageSource::Npm => NPM_COLOR,
                PackageSource::Pip => PIP_COLOR,
            };

            let source_label = if pkg.is_aur {
                "AUR".to_string()
            } else {
                pkg.source.label().to_string()
            };
            let date_str = pkg
                .install_date
                .and_then(|ts| {
                    chrono::Utc
                        .timestamp_opt(ts, 0)
                        .single()
                        .map(|dt| dt.format("%Y-%m-%d").to_string())
                })
                .unwrap_or_else(|| "—".to_string());

            Row::new(vec![
                Cell::from(name),
                Cell::from(version),
                Cell::from(source_label).style(Style::new().fg(source_color)),
                Cell::from(date_str),
            ])
            .height(1)
        })
        .collect();

    let widths = [
        Constraint::Length(name_width),
        Constraint::Length(24),
        Constraint::Length(8),
        Constraint::Length(12),
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
        .highlight_symbol("▶ ");

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

fn draw_urlbar(f: &mut Frame, area: Rect, app: &App) {
    let url_text = if app.filtered_indices.is_empty() || app.selected_index >= app.filtered_indices.len() {
        String::new()
    } else {
        let pkg_idx = app.filtered_indices[app.selected_index];
        let pkg = &app.packages[pkg_idx];
        match &pkg.url {
            Some(url) => format!(" {} URL: {}  (o to open) ", pkg.source.label(), url),
            None => String::new(),
        }
    };

    let paragraph = Paragraph::new(url_text)
        .style(Style::new().bg(Color::DarkGray).fg(Color::Yellow));
    f.render_widget(paragraph, area);
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    let mut parts = Vec::new();

    if let SourceFilter::All = app.source_filter {
        for source in PackageSource::all() {
            if let Some(count) = app.source_counts.get(source) {
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
            " {} — showing {} | {} ",
            parts.join(" "),
            app.filtered_indices.len(),
            app.status_message
        )
    };

    let paragraph = Paragraph::new(status)
        .style(Style::new().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(paragraph, area);
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
