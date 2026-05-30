use crate::Commands;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
use std::io::{stdout, Write};

pub async fn run_main_menu() -> anyhow::Result<Option<Commands>> {
    let mut stdout = stdout();
    enable_raw_mode()?;
    execute!(stdout, Hide)?;

    let options = [
        "Receive Files (From Phone)",
        "Share Files (To Phone)",
        "Pair New Device",
        "List Paired Devices",
        "Setup Hotspot (Direct Connection)",
        "Exit",
    ];

    let mut selected = 0;

    loop {
        execute!(stdout, Clear(ClearType::All), MoveTo(0, 0))?;
        execute!(
            stdout,
            SetForegroundColor(Color::Green),
            Print("\r\n  ╔══════════════════════════════════════════════════════════════════╗\r\n"),
            Print("  ║                                                                  ║\r\n"),
            Print("  ║               [ FILEDROP ]  —  MAIN MENU                         ║\r\n"),
            Print("  ║                                                                  ║\r\n"),
            Print("  ╚══════════════════════════════════════════════════════════════════╝\r\n\r\n"),
            ResetColor
        )?;

        for (i, option) in options.iter().enumerate() {
            if i == selected {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Black),
                    SetBackgroundColor(Color::Green),
                    Print(format!("    > {:<60} \r\n", option)),
                    ResetColor
                )?;
            } else {
                execute!(stdout, Print(format!("      {:<60} \r\n", option)))?;
            }
        }

        execute!(
            stdout,
            Print("\r\n  Use Up/Down arrows to navigate, Enter to select.\r\n")
        )?;

        stdout.flush()?;

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if selected > 0 {
                            selected -= 1;
                        } else {
                            selected = options.len() - 1;
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if selected < options.len() - 1 {
                            selected += 1;
                        } else {
                            selected = 0;
                        }
                    }
                    KeyCode::Enter => {
                        break;
                    }
                    KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                        selected = options.len() - 1; // Exit option
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    disable_raw_mode()?;
    execute!(stdout, Show, Clear(ClearType::All), MoveTo(0, 0))?;

    match selected {
        0 => Ok(Some(Commands::Receive {
            mode: None,
            multi: false,
            encrypt: false,
        })),
        1 => Ok(Some(Commands::Share {
            path: None,
            expires: None,
            once: false,
            pin: None,
            mode: None,
        })),
        2 => Ok(Some(Commands::Pair)),
        3 => Ok(Some(Commands::Peers)),
        4 => Ok(Some(Commands::Hotspot { auto: false })),
        _ => Ok(None),
    }
}
