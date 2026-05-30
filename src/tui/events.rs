//! Keyboard and terminal event handling for the TUI.
//!
//! Supports multiple screen contexts: main TUI, mode selection,
//! hotspot guide, and file browser.

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
    /// Scroll log panel up (one page)
    LogScrollUp,
    /// Scroll log panel down (one page)
    LogScrollDown,
    /// Jump to top of queue
    ScrollHome,
    /// Jump to bottom of queue
    ScrollEnd,
    /// Periodic tick for updates
    #[allow(dead_code)]
    Tick,

    // ── Feature 2A: File Browser Events ─────────────────────────
    /// Switch focus between transfer queue and file browser
    TabFocus,
    /// Enter directory or confirm (Enter key)
    Enter,
    /// Go up one directory (Backspace)
    GoBack,
    /// Toggle file/folder selection (Space)
    ToggleSelect,
    /// Select all items in current directory
    SelectAll,
    /// Send selected files to phone
    SendSelected,
    /// Clear selection (Esc in file browser context)
    ClearSelection,

    // ── Feature 3: Mode Selection / Hotspot Events ──────────────
    /// Select option 1 (Router mode)
    SelectOption1,
    /// Select option 2 (Hotspot mode)
    SelectOption2,
    /// Skip prompt (use last saved choice)
    SkipPrompt,
    /// Auto-create hotspot (Linux only)
    AutoSetup,
    /// Confirm / "Yes, done" on hotspot guide
    ConfirmYes,
    /// Go back to previous screen
    GoBackScreen,
}

/// Current input context for key mapping
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputContext {
    /// Main TUI with transfer queue and log
    MainTui,
    /// File browser is focused (right pane)
    FileBrowser,
    /// Connection mode selection screen
    ModeSelection,
    /// Hotspot setup guide screen
    HotspotGuide,
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

/// Map a crossterm key event to an application event.
///
/// This mapping covers ALL contexts. The app state machine decides
/// which events are relevant based on the current screen/focus.
fn map_key_event(key: KeyEvent) -> Option<AppEvent> {
    match key.code {
        // ── Universal ───────────────────────────────────────────
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(AppEvent::Quit),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(AppEvent::Quit)
        }

        // ── Navigation ──────────────────────────────────────────
        KeyCode::Up | KeyCode::Char('k') => Some(AppEvent::ScrollUp),
        KeyCode::Down | KeyCode::Char('j') => Some(AppEvent::ScrollDown),
        KeyCode::Home => Some(AppEvent::ScrollHome),
        KeyCode::End => Some(AppEvent::ScrollEnd),
        KeyCode::PageUp => Some(AppEvent::LogScrollUp),
        KeyCode::PageDown => Some(AppEvent::LogScrollDown),

        // ── File Browser (Feature 2A) ───────────────────────────
        KeyCode::Tab => Some(AppEvent::TabFocus),
        KeyCode::Enter => Some(AppEvent::Enter),
        KeyCode::Backspace => Some(AppEvent::GoBack),
        KeyCode::Char(' ') => Some(AppEvent::ToggleSelect),
        KeyCode::Char('a') | KeyCode::Char('A') => Some(AppEvent::SelectAll),
        KeyCode::Char('s') | KeyCode::Char('S') => Some(AppEvent::SendSelected),
        KeyCode::Esc => Some(AppEvent::ClearSelection),

        // ── Mode Selection (Feature 3) ──────────────────────────
        KeyCode::Char('1') => Some(AppEvent::SelectOption1),
        KeyCode::Char('2') => Some(AppEvent::SelectOption2),
        KeyCode::Char('y') | KeyCode::Char('Y') => Some(AppEvent::ConfirmYes),
        KeyCode::Char('b') | KeyCode::Char('B') => Some(AppEvent::GoBackScreen),

        // H/E for home/end (existing behavior from queue scroll)
        KeyCode::Char('h') | KeyCode::Char('H') => Some(AppEvent::ScrollHome),
        KeyCode::Char('e') | KeyCode::Char('E') => Some(AppEvent::ScrollEnd),

        _ => None,
    }
}
