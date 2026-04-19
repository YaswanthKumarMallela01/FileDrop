//! TUI rendering — Ratatui widget composition.
//!
//! Implements the exact layout from the spec:
//! ┌─────────────────────────────────────────────┐
//! │  FileDrop  v0.1  [RECEIVE MODE]  📡 Ready   │  ← Header bar
//! ├────────────────────┬────────────────────────┤
//! │  FILE QUEUE        │  TRANSFER LOG          │
//! │  ─────────────     │  ─────────────         │
//! │  > photo.jpg  2MB  │  [12:04:01] Connected  │
//! │    video.mp4 800MB │  [12:04:02] Receiving  │
//! │    doc.pdf   400KB │    photo.jpg...        │
//! │                    │  [12:04:05] Done ✓     │
//! ├────────────────────┴────────────────────────┤
//! │  ████████████████░░░░░░░  68%  2.1 MB/s     │  ← Progress bar
//! │  Speed: ▂▃▅▆▇▇▅▄▃▂  (sparkline)            │
//! │  Ctrl+C to cancel  |  Q to quit             │  ← Keybinds hint
//! └─────────────────────────────────────────────┘
//!
//! Colors from the Stitch design system:
//! - Background: terminal default (transparent)
//! - Header: Bold white text, teal/cyan border
//! - Active file: Bold cyan
//! - Done files: Dim green with ✓
//! - Progress filled: Cyan / unfilled: Dark gray
//! - Log timestamps: Dim gray; messages: white
//! - Error messages: Bold red

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline},
    Frame,
};

use crate::transfer::protocol::{self, TransferStatus};

use super::app::{AppState, LogLevel};

// ── Design System Colors (from Stitch) ──────────────────────────────────────

/// Primary cyan accent — #00D4FF
const CYAN: Color = Color::Rgb(0, 212, 255);
/// Surface container low — used for subtle element backgrounds
#[allow(dead_code)]
const SURFACE_LOW: Color = Color::Rgb(25, 27, 34);
/// Surface container high — elevated panels
#[allow(dead_code)]
const SURFACE_HIGH: Color = Color::Rgb(40, 42, 48);
/// Muted text / timestamps
const MUTED: Color = Color::Rgb(138, 143, 152);
/// Success green — completed transfers
const SUCCESS_GREEN: Color = Color::Rgb(78, 222, 163);
/// Error red — failed transfers
const ERROR_RED: Color = Color::Rgb(255, 180, 171);
/// Warning amber
const WARNING_AMBER: Color = Color::Rgb(255, 185, 90);
/// Dark gray for unfilled progress
const DARK_GRAY: Color = Color::Rgb(60, 73, 78);
/// On-surface text (not pure white)
const TEXT_PRIMARY: Color = Color::Rgb(226, 226, 235);
/// Dim text for secondary info
const TEXT_DIM: Color = Color::Rgb(133, 147, 152);
/// Border color — ghost border
const BORDER: Color = Color::Rgb(60, 73, 78);
/// Bright border for active elements
const BORDER_ACTIVE: Color = CYAN;

// ── Main Render Function ────────────────────────────────────────────────────

/// Render the entire TUI frame
pub fn render(frame: &mut Frame, app: &AppState) {
    let area = frame.area();

    // Main vertical layout:
    //   [1] Header bar (3 lines — border + text + border)
    //   [2] Middle content (file queue + log side by side)
    //   [3] Bottom panel (progress + sparkline + keybinds)
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),   // Middle content
            Constraint::Length(6), // Bottom panel
        ])
        .split(area);

    render_header(frame, main_layout[0], app);
    render_middle(frame, main_layout[1], app);
    render_bottom(frame, main_layout[2], app);
}

// ── Header Bar ──────────────────────────────────────────────────────────────

fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let mode_text = app.mode.to_string();
    let status_text = app.status.to_string();

    let mut spans = vec![
        Span::styled("  FileDrop ", Style::default().fg(CYAN).bold()),
        Span::styled("v0.1 ", Style::default().fg(TEXT_DIM)),
        Span::styled(" [", Style::default().fg(DARK_GRAY)),
        Span::styled(
            mode_text,
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
        Span::styled("] ", Style::default().fg(DARK_GRAY)),
        Span::styled("  ", Style::default()),
        Span::styled(status_text, Style::default().fg(TEXT_PRIMARY)),
    ];

    // Show phone URL in header if available (receive mode)
    if let Some(ref url) = app.phone_url {
        spans.push(Span::styled("  │  ", Style::default().fg(DARK_GRAY)));
        spans.push(Span::styled("📱 ", Style::default()));
        spans.push(Span::styled(
            url.clone(),
            Style::default()
                .fg(SUCCESS_GREEN)
                .add_modifier(Modifier::BOLD),
        ));
    }

    let header_line = Line::from(spans);

    let header = Paragraph::new(header_line).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_ACTIVE))
            .style(Style::default()),
    );

    frame.render_widget(header, area);
}

// ── Middle Content (File Queue + Transfer Log) ──────────────────────────────

fn render_middle(frame: &mut Frame, area: Rect, app: &AppState) {
    // Horizontal split: 40% file queue, 60% transfer log
    let middle_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_file_queue(frame, middle_layout[0], app);
    render_transfer_log(frame, middle_layout[1], app);
}

fn render_file_queue(frame: &mut Frame, area: Rect, app: &AppState) {
    let items: Vec<ListItem> = app
        .file_queue
        .iter()
        .enumerate()
        .map(|(_i, file)| {
            let (prefix, style) = match &file.status {
                TransferStatus::InProgress => (
                    "▶ ",
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                ),
                TransferStatus::Completed => (
                    "✓ ",
                    Style::default()
                        .fg(SUCCESS_GREEN)
                        .add_modifier(Modifier::DIM),
                ),
                TransferStatus::Failed(_) => (
                    "✗ ",
                    Style::default()
                        .fg(ERROR_RED)
                        .add_modifier(Modifier::BOLD),
                ),
                TransferStatus::Queued => ("  ", Style::default().fg(TEXT_DIM)),
                TransferStatus::Cancelled => (
                    "⊘ ",
                    Style::default()
                        .fg(WARNING_AMBER)
                        .add_modifier(Modifier::DIM),
                ),
            };

            let size = file.size_display();
            let progress_str = if file.status == TransferStatus::InProgress {
                format!(" {:.0}%", file.progress() * 100.0)
            } else {
                String::new()
            };

            let line = Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(&file.name, style),
                Span::styled("  ", Style::default()),
                Span::styled(size, Style::default().fg(MUTED)),
                Span::styled(progress_str, Style::default().fg(CYAN)),
            ]);

            ListItem::new(line)
        })
        .collect();

    // Show placeholder if empty
    let list = if items.is_empty() {
        let empty_items = vec![
            ListItem::new(Line::from(vec![
                Span::styled("  No files in queue", Style::default().fg(TEXT_DIM)),
            ])),
            ListItem::new(Line::from(vec![Span::styled(
                "  Waiting for transfer...",
                Style::default().fg(MUTED),
            )])),
        ];
        List::new(empty_items)
    } else {
        List::new(items)
    };

    let block = Block::default()
        .title(Span::styled(
            " FILE QUEUE ",
            Style::default().fg(CYAN).bold(),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));

    let list = list.block(block);
    frame.render_widget(list, area);
}

fn render_transfer_log(frame: &mut Frame, area: Rect, app: &AppState) {
    let items: Vec<ListItem> = app
        .log_entries
        .iter()
        .map(|entry| {
            let (msg_color, prefix_icon) = match entry.level {
                LogLevel::Info => (TEXT_PRIMARY, ""),
                LogLevel::Success => (SUCCESS_GREEN, "✓ "),
                LogLevel::Warning => (WARNING_AMBER, "⚠ "),
                LogLevel::Error => (ERROR_RED, "✗ "),
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("  [{}] ", entry.timestamp),
                    Style::default().fg(MUTED),
                ),
                Span::styled(prefix_icon, Style::default().fg(msg_color)),
                Span::styled(&entry.message, Style::default().fg(msg_color)),
            ]);

            ListItem::new(line)
        })
        .collect();

    // Show placeholder if empty
    let list = if items.is_empty() {
        List::new(vec![ListItem::new(Line::from(vec![Span::styled(
            "  No log entries yet",
            Style::default().fg(TEXT_DIM),
        )]))])
    } else {
        List::new(items)
    };

    let block = Block::default()
        .title(Span::styled(
            " TRANSFER LOG ",
            Style::default().fg(CYAN).bold(),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));

    let list = list.block(block);
    frame.render_widget(list, area);
}

// ── Bottom Panel (Progress + Sparkline + Keybinds) ──────────────────────────

fn render_bottom(frame: &mut Frame, area: Rect, app: &AppState) {
    let bottom_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // Progress bar
            Constraint::Length(2), // Sparkline
            Constraint::Length(2), // Keybinds
        ])
        .split(area);

    render_progress_bar(frame, bottom_layout[0], app);
    render_sparkline(frame, bottom_layout[1], app);
    render_keybinds(frame, bottom_layout[2], app);
}

fn render_progress_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let percentage = (app.overall_progress * 100.0).min(100.0) as u16;
    let speed_str = protocol::format_speed(app.current_speed);

    let transferred = protocol::format_bytes(app.total_bytes_transferred);
    let total = protocol::format_bytes(app.total_bytes_expected);

    let label = format!(
        "{}%  {}  ({} / {})",
        percentage, speed_str, transferred, total
    );

    let gauge = Gauge::default()
        .block(Block::default())
        .gauge_style(
            Style::default()
                .fg(CYAN)
                .bg(DARK_GRAY)
                .add_modifier(Modifier::BOLD),
        )
        .percent(percentage)
        .label(Span::styled(label, Style::default().fg(TEXT_PRIMARY).bold()));

    frame.render_widget(gauge, area);
}

fn render_sparkline(frame: &mut Frame, area: Rect, app: &AppState) {
    // Normalize speed history to fit sparkline height
    let max_speed = app.speed_history.iter().copied().max().unwrap_or(1).max(1);

    let sparkline = Sparkline::default()
        .block(Block::default().title(Span::styled(
            " Speed ",
            Style::default().fg(TEXT_DIM),
        )))
        .data(&app.speed_history)
        .max(max_speed)
        .style(Style::default().fg(CYAN));

    frame.render_widget(sparkline, area);
}

fn render_keybinds(frame: &mut Frame, area: Rect, _app: &AppState) {
    let keybinds = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("Ctrl+C", Style::default().fg(CYAN).bold()),
        Span::styled(" cancel  │  ", Style::default().fg(TEXT_DIM)),
        Span::styled("Q", Style::default().fg(CYAN).bold()),
        Span::styled(" quit  │  ", Style::default().fg(TEXT_DIM)),
        Span::styled("↑↓", Style::default().fg(CYAN).bold()),
        Span::styled(" scroll queue  │  ", Style::default().fg(TEXT_DIM)),
        Span::styled("PgUp/PgDn", Style::default().fg(CYAN).bold()),
        Span::styled(" scroll log", Style::default().fg(TEXT_DIM)),
    ]);

    let paragraph = Paragraph::new(keybinds).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER)),
    );

    frame.render_widget(paragraph, area);
}
