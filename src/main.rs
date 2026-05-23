//! Binary entrypoint and event loop for the `inventory` TUI.
//!
//! Module layout:
//! - `app` - central application state, filter/sort logic, package load.
//! - `package` - the `Package` row struct and `PackageSource` enum.
//! - `collectors` - one module per package manager; each implements `Collector`
//!   and turns a tool's output into `Vec<Package>` during `App::load`.
//! - `details` - lazy per-package detail fetcher behind the `d` key.
//! - `ui` - all ratatui drawing code; no mutation of `App`.
//!
//! Control flow at runtime:
//! 1. `main` enters the alternate screen + raw mode and calls `App::load`
//!    to populate the package list (parallel across collectors).
//! 2. `run` blocks on `event::read` and routes keystrokes by `app.input_mode`.
//! 3. Some actions (uninstall) suspend the TUI, shell out, then restore it.

mod app;
mod collectors;
mod details;
mod package;
mod ui;

use app::{App, InputMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::{CrosstermBackend, Terminal};
use std::io;
use std::process::Command;

fn main() -> io::Result<()> {
    // Standard ratatui setup: alternate screen + raw mode so we own the terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::default();
    app.load();

    let result = run(&mut terminal, &mut app);

    // Always restore the terminal, even if `run` returned an error.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        // 6 = header bar + help bar + status bar + table top border + header row + bottom border.
        // Must match what draw_table actually renders, or the bottom row can drift off-screen
        // while scroll_to_selection still thinks it's in view.
        let list_height = (terminal.size()?.height as usize).saturating_sub(6);
        app.scroll_to_selection(list_height);

        terminal.draw(|f| ui::draw(f, app))?;

        let Event::Key(key) = event::read()? else { continue };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.input_mode {
            InputMode::Search => match key.code {
                KeyCode::Esc => app.input_mode = InputMode::Normal,
                // Enter just leaves search mode - the filter is already applied
                // because every keystroke below refilters live.
                KeyCode::Enter => app.input_mode = InputMode::Normal,
                // Arrow keys (and PageUp/Down) drop the user out of search and
                // immediately apply the navigation, so they can scan results
                // without having to press Enter or Esc first. The query stays
                // intact so the filtered view is preserved.
                KeyCode::Up => {
                    app.input_mode = InputMode::Normal;
                    app.select_prev();
                }
                KeyCode::Down => {
                    app.input_mode = InputMode::Normal;
                    app.select_next();
                }
                KeyCode::PageUp => {
                    app.input_mode = InputMode::Normal;
                    app.page_up(list_height);
                }
                KeyCode::PageDown => {
                    app.input_mode = InputMode::Normal;
                    app.page_down(list_height);
                }
                KeyCode::Char(c) => {
                    app.search_query.push(c);
                    app.update_filtered();
                }
                KeyCode::Backspace => {
                    app.search_query.pop();
                    app.update_filtered();
                }
                _ => {}
            },
            InputMode::UninstallConfirm => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(cmd) = app.uninstall_command() {
                        run_uninstall(terminal, &cmd)?;
                        app.load();
                    }
                    app.input_mode = InputMode::Normal;
                }
                // Any other key cancels - matches the popup hint.
                _ => app.input_mode = InputMode::Normal,
            },
            // Details is read-only: any key dismisses the overlay.
            InputMode::Details => app.close_details(),
            InputMode::Normal => match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('/') => {
                    app.input_mode = InputMode::Search;
                    app.search_query.clear();
                }
                KeyCode::Char('j') | KeyCode::Down => app.select_next(),
                KeyCode::Char('k') | KeyCode::Up => app.select_prev(),
                KeyCode::Char('J') => {
                    for _ in 0..5 {
                        app.select_next();
                    }
                }
                KeyCode::Char('K') => {
                    for _ in 0..5 {
                        app.select_prev();
                    }
                }
                KeyCode::Char('d') if key.modifiers == KeyModifiers::CONTROL => {
                    app.page_down(list_height / 2);
                }
                KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                    app.page_up(list_height / 2);
                }
                KeyCode::PageDown => app.page_down(list_height),
                KeyCode::PageUp => app.page_up(list_height),
                KeyCode::Home | KeyCode::Char('g') => app.select_first(),
                KeyCode::End | KeyCode::Char('G') => app.select_last(list_height),
                KeyCode::Tab => app.cycle_source_filter(),
                KeyCode::Char('e') => app.toggle_explicit(),
                KeyCode::Char('o') => app.toggle_omarchy(),
                KeyCode::Enter => app.open_selected_url(),
                KeyCode::Char('s') => app.cycle_sort(),
                KeyCode::Char('R') => app.load(),
                KeyCode::Char('d') => app.open_details(),
                KeyCode::Char('X') if app.selected_package().is_some() => {
                    app.input_mode = InputMode::UninstallConfirm;
                }
                KeyCode::Esc => {
                    app.search_query.clear();
                    app.update_filtered();
                }
                _ => {}
            },
        }
    }
}

/// Suspend the TUI, run the package-manager command so the user can see
/// its output (and respond to sudo / confirmation prompts), then restore
/// the TUI cleanly.
fn run_uninstall(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    cmd: &[String],
) -> io::Result<()> {
    // Drop back to the user's normal terminal so prompts render naturally.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let mut parts = cmd.iter();
    if let Some(program) = parts.next() {
        let _ = Command::new(program).args(parts).status();
    }

    // Re-enter the alternate screen. `terminal.clear()` is the critical bit:
    // ratatui caches the previous frame and only writes diffs against it, so
    // without an explicit clear the next draw inherits garbage from whatever
    // the package manager printed and leaves the layout fragmented.
    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.clear()?;
    Ok(())
}
