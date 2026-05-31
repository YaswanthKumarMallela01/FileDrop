use crate::Commands;
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Terminal,
};
use std::io::stdout;

pub enum MenuAction {
    RunCommand(Commands),
    Install,
    Exit,
}

pub async fn run_main_menu() -> anyhow::Result<MenuAction> {
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let options = vec![
        "Receive Files (From Phone)",
        "Share Files (To Phone)",
        "Pair New Device",
        "List Paired Devices",
        "Setup Hotspot (Direct Connection)",
        "Install FileDrop (Add to System PATH)",
        "Exit",
    ];

    let mut selected_index = 0;

    loop {
        terminal.draw(|f| {
            let size = f.area();

            let vertical_layout = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(2),  // Top margin
                    Constraint::Length(7),  // Header
                    Constraint::Length(12), // Menu
                    Constraint::Min(0),     // Bottom
                ])
                .split(size);

            let horizontal_layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(10),
                    Constraint::Percentage(80),
                    Constraint::Percentage(10),
                ])
                .split(vertical_layout[1]);

            let header_area = horizontal_layout[1];

            let header_text = vec![
                Line::from(Span::styled("╔══════════════════════════════════════════════════════════════════╗", Style::default().fg(Color::Green))),
                Line::from(Span::styled("║                                                                  ║", Style::default().fg(Color::Green))),
                Line::from(vec![
                    Span::styled("║               ", Style::default().fg(Color::Green)),
                    Span::styled("[ FILEDROP ]  —  MAIN MENU", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                    Span::styled("                         ║", Style::default().fg(Color::Green)),
                ]),
                Line::from(Span::styled("║                                                                  ║", Style::default().fg(Color::Green))),
                Line::from(Span::styled("╚══════════════════════════════════════════════════════════════════╝", Style::default().fg(Color::Green))),
            ];

            let header = Paragraph::new(header_text).alignment(Alignment::Center);
            f.render_widget(header, header_area);

            let menu_horizontal = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(20),
                    Constraint::Percentage(60),
                    Constraint::Percentage(20),
                ])
                .split(vertical_layout[2]);

            let menu_area = menu_horizontal[1];

            let items: Vec<ListItem> = options
                .iter()
                .enumerate()
                .map(|(i, &opt)| {
                    if i == selected_index {
                        ListItem::new(format!("  > {} ", opt)).style(
                            Style::default()
                                .fg(Color::Black)
                                .bg(Color::Green)
                                .add_modifier(Modifier::BOLD),
                        )
                    } else {
                        ListItem::new(format!("    {} ", opt)).style(Style::default().fg(Color::White))
                    }
                })
                .collect();

            let list = List::new(items).block(Block::default().borders(Borders::NONE));
            f.render_widget(list, menu_area);

            let footer_area = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(10),
                    Constraint::Percentage(80),
                    Constraint::Percentage(10),
                ])
                .split(vertical_layout[3])[1];

            let footer = Paragraph::new("Use Up/Down arrows to navigate, Enter to select.")
                .alignment(Alignment::Center)
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(footer, footer_area);
        })?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if selected_index > 0 {
                            selected_index -= 1;
                        } else {
                            selected_index = options.len() - 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if selected_index < options.len() - 1 {
                            selected_index += 1;
                        } else {
                            selected_index = 0;
                        }
                    }
                    KeyCode::Enter => {
                        break;
                    }
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                        selected_index = options.len() - 1; // Exit
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    match selected_index {
        0 => Ok(MenuAction::RunCommand(Commands::Receive {
            mode: None,
            multi: false,
            encrypt: false,
        })),
        1 => Ok(MenuAction::RunCommand(Commands::Share {
            path: None,
            expires: None,
            once: false,
            pin: None,
            mode: None,
        })),
        2 => Ok(MenuAction::RunCommand(Commands::Pair)),
        3 => Ok(MenuAction::RunCommand(Commands::Peers)),
        4 => Ok(MenuAction::RunCommand(Commands::Hotspot { auto: false })),
        5 => Ok(MenuAction::Install),
        _ => Ok(MenuAction::Exit),
    }
}
