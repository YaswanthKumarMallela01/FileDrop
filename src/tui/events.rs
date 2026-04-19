//! Keyboard and terminal event handling for the TUI.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

/// Application events triggered by user input
#[derive(Debug, Clone)]
pub enum AppEvent {
    /// User requested quit (Q or Ctrl+C)
    Quit,
    /// Scroll file queue up
    ScrollUp,
    /// Scroll file queue down
    ScrollDown,
    /// Scroll log panel up
    LogScrollUp,
    /// Scroll log panel down
    LogScrollDown,
    /// Periodic tick for updates
    #[allow(dead_code)]
    Tick,
}

/// Poll for keyboard events and send them through the channel.
/// Runs in a separate tokio task.
pub async fn poll_keyboard_events(tx: mpsc::UnboundedSender<AppEvent>) {
    loop {
        // Poll with 50ms timeout to remain responsive
        match tokio::task::spawn_blocking(|| {
            event::poll(std::time::Duration::from_millis(50))
        })
        .await
        {
            Ok(Ok(true)) => {
                match tokio::task::spawn_blocking(event::read).await {
                    Ok(Ok(Event::Key(key))) => {
                        if let Some(app_event) = map_key_event(key) {
                            if tx.send(app_event).is_err() {
                                return; // Channel closed, exit
                            }
                        }
                    }
                    Ok(Ok(_)) => {} // Ignore mouse, resize, etc.
                    Ok(Err(_)) => {} // Read error
                    Err(_) => {}     // Task join error
                }
            }
            Ok(Ok(false)) => {
                // No event available, continue polling
            }
            Ok(Err(_)) | Err(_) => {
                // Poll error or task error
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
    }
}

/// Map a crossterm key event to an application event
fn map_key_event(key: KeyEvent) -> Option<AppEvent> {
    match key.code {
        // Quit: Q or Ctrl+C
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(AppEvent::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(AppEvent::Quit)
        }

        // File queue navigation
        KeyCode::Up | KeyCode::Char('k') => Some(AppEvent::ScrollUp),
        KeyCode::Down | KeyCode::Char('j') => Some(AppEvent::ScrollDown),

        // Log panel navigation
        KeyCode::PageUp => Some(AppEvent::LogScrollUp),
        KeyCode::PageDown => Some(AppEvent::LogScrollDown),

        // Escape also quits
        KeyCode::Esc => Some(AppEvent::Quit),

        _ => None,
    }
}
