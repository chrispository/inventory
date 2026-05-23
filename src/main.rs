mod app;
mod collectors;
mod package;
mod ui;

use app::{App, InputMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::{CrosstermBackend, Terminal};
use std::io;

fn main() -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::default();

    app.load();

    let result = run(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> io::Result<()> {
    loop {
        let list_height = terminal.size()?.height.saturating_sub(6) as usize;

        app.scroll_to_selection(list_height);

        terminal.draw(|f| ui::draw(f, app))?;

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }

            match app.input_mode {
                InputMode::Search => match key.code {
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                    }
                    KeyCode::Enter => {
                        app.input_mode = InputMode::Normal;
                        app.update_filtered();
                    }
                    KeyCode::Char(c) => {
                        app.search_query.push(c);
                    }
                    KeyCode::Backspace => {
                        app.search_query.pop();
                    }
                    _ => {}
                },
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => return Ok(()),
                    KeyCode::Char('/') => {
                        app.input_mode = InputMode::Search;
                        app.search_query.clear();
                    }
                    KeyCode::Char('j') | KeyCode::Down => {
                        app.select_next();
                    }
                    KeyCode::Char('k') | KeyCode::Up => {
                        app.select_prev();
                    }
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
                    KeyCode::Char('d') if key.modifiers == event::KeyModifiers::CONTROL => {
                        app.page_down(list_height / 2);
                    }
                    KeyCode::Char('u') if key.modifiers == event::KeyModifiers::CONTROL => {
                        app.page_up(list_height / 2);
                    }
                    KeyCode::PageDown => {
                        app.page_down(list_height);
                    }
                    KeyCode::PageUp => {
                        app.page_up(list_height);
                    }
                    KeyCode::Home | KeyCode::Char('g') => {
                        app.select_first();
                    }
                    KeyCode::End | KeyCode::Char('G') => {
                        app.select_last(list_height);
                    }
                    KeyCode::Tab => {
                        app.cycle_source_filter();
                    }
                    KeyCode::Char('e') => {
                        app.toggle_explicit();
                    }
                    KeyCode::Char('o') | KeyCode::Enter => {
                        app.open_selected_url();
                    }
                    KeyCode::Char('s') => {
                        app.cycle_sort_column();
                    }
                    KeyCode::Char('r') => {
                        app.toggle_sort();
                    }
                    KeyCode::Char('R') => {
                        app.load();
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
}
