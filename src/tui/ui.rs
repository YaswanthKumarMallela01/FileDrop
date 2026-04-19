//! TUI rendering — Hacker-themed Ratatui widget composition.
//!
//! ╔═══════════════════════════════════════════════════╗
//! ║  [FILEDROP] v0.1  ::  RECEIVE_MODE  ::  ONLINE   ║
//! ╠═════════════════════╤═════════════════════════════╣
//! ║  [ TRANSFER QUEUE ] │  [ SYSTEM LOG ]             ║
//! ║  ─────────────────  │  ───────────────            ║
//! ║  > photo.jpg  2MB   │  [09:41:31] SYS: READY      ║
//! ║    video.mp4 800MB  │  [09:41:32] RX: Incoming     ║
//! ║    doc.pdf   400KB  │  [09:41:35] OK: Verified ✓   ║
//! ╠═════════════════════╧═════════════════════════════╣
//! ║  ████████████░░░░░░░  68%  2.1 MB/s  ETA 1m 32s   ║
//! ║  Speed: ▂▃▅▆▇▇▅▄▃▂                                ║
//! ║  [CTRL+C] abort  [Q] exit  [↑↓] scroll            ║
//! ╚═══════════════════════════════════════════════════╝

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Sparkline},
    Frame,
};

use crate::transfer::protocol::{self, TransferStatus};

use super::app::{AppState, LogLevel};

// ── Hacker Theme Colors ─────────────────────────────────────────────────────

/// Matrix green — primary accent
const GREEN: Color = Color::Rgb(0, 255, 65);
/// Dim green for completed items
const GREEN_DIM: Color = Color::Rgb(0, 120, 40);
/// Bright green for active elements
const GREEN_BRIGHT: Color = Color::Rgb(100, 255, 130);
/// Error red
const ERROR_RED: Color = Color::Rgb(255, 0, 51);
/// Warning amber
const WARNING_AMBER: Color = Color::Rgb(255, 176, 0);
/// Dark gray for unfilled progress
const DARK_GRAY: Color = Color::Rgb(30, 30, 30);
/// Primary text
const TEXT_PRIMARY: Color = Color::Rgb(180, 180, 180);
/// Dim text
const TEXT_DIM: Color = Color::Rgb(100, 100, 100);
/// Muted text
const MUTED: Color = Color::Rgb(85, 85, 85);
/// Border color
const BORDER: Color = Color::Rgb(40, 40, 40);
/// Active border
const BORDER_ACTIVE: Color = GREEN;
/// Selected/highlighted item background
const HIGHLIGHT_BG: Color = Color::Rgb(20, 40, 20);

// ── Main Render Function ────────────────────────────────────────────────────

/// Render the entire TUI frame
pub fn render(frame: &mut Frame, app: &AppState) {
    let area = frame.area();

    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),   // Middle content
            Constraint::Length(7), // Bottom panel (progress + sparkline + keybinds)
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
        Span::styled("  [", Style::default().fg(MUTED)),
        Span::styled(
            "FILEDROP",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ),
        Span::styled("] ", Style::default().fg(MUTED)),
        Span::styled("v0.1 ", Style::default().fg(TEXT_DIM)),
        Span::styled(" :: ", Style::default().fg(MUTED)),
        Span::styled(
            mode_text,
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" :: ", Style::default().fg(MUTED)),
        Span::styled(status_text, Style::default().fg(GREEN_BRIGHT)),
    ];

    // Show phone URL in header if available
    if let Some(ref url) = app.phone_url {
        spans.push(Span::styled("  |  ", Style::default().fg(MUTED)));
        spans.push(Span::styled(
            url.clone(),
            Style::default()
                .fg(GREEN)
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
    let middle_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    render_file_queue(frame, middle_layout[0], app);
    render_transfer_log(frame, middle_layout[1], app);
}

fn render_file_queue(frame: &mut Frame, area: Rect, app: &AppState) {
    let inner_height = area.height.saturating_sub(2) as usize; // minus borders

    let items: Vec<ListItem> = app
        .file_queue
        .iter()
        .enumerate()
        .map(|(i, file)| {
            let is_selected = i == app.queue_scroll;

            let (prefix, style) = match &file.status {
                TransferStatus::InProgress => (
                    "▶ ",
                    Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
                ),
                TransferStatus::Completed => (
                    "✓ ",
                    Style::default().fg(GREEN_DIM),
                ),
                TransferStatus::Failed(_) => (
                    "✗ ",
                    Style::default()
                        .fg(ERROR_RED)
                        .add_modifier(Modifier::BOLD),
                ),
                TransferStatus::Queued => ("  ", Style::default().fg(TEXT_DIM)),
                TransferStatus::Cancelled => (
                    "- ",
                    Style::default().fg(WARNING_AMBER),
                ),
            };

            let size = file.size_display();
            let progress_str = if file.status == TransferStatus::InProgress {
                format!(" {:.0}%", file.progress() * 100.0)
            } else {
                String::new()
            };

            let mut base_style = style;
            if is_selected {
                base_style = base_style.bg(HIGHLIGHT_BG);
            }

            let line = Line::from(vec![
                Span::styled(if is_selected { "> " } else { "  " }, Style::default().fg(GREEN)),
                Span::styled(prefix, base_style),
                Span::styled(&file.name, base_style),
                Span::styled("  ", Style::default()),
                Span::styled(size, Style::default().fg(MUTED)),
                Span::styled(progress_str, Style::default().fg(GREEN)),
            ]);

            ListItem::new(line)
        })
        .collect();

    // Apply scroll offset — show only visible items
    let total = items.len();
    let start = if total > inner_height {
        // Center the cursor in the viewport
        let half = inner_height / 2;
        if app.queue_scroll < half {
            0
        } else if app.queue_scroll + half >= total {
            total.saturating_sub(inner_height)
        } else {
            app.queue_scroll.saturating_sub(half)
        }
    } else {
        0
    };
    let end = (start + inner_height).min(total);
    let visible: Vec<ListItem> = items.into_iter().skip(start).take(end - start).collect();

    let list = if visible.is_empty() {
        let empty_items = vec![
            ListItem::new(Line::from(vec![
                Span::styled("  // No targets in queue", Style::default().fg(TEXT_DIM)),
            ])),
            ListItem::new(Line::from(vec![Span::styled(
                "  // Awaiting incoming stream...",
                Style::default().fg(MUTED),
            )])),
        ];
        List::new(empty_items)
    } else {
        List::new(visible)
    };

    let scroll_indicator = if total > inner_height {
        format!(" [ TRANSFER QUEUE ] ({}/{}) ", app.queue_scroll + 1, total)
    } else if total > 0 {
        format!(" [ TRANSFER QUEUE ] ({}) ", total)
    } else {
        " [ TRANSFER QUEUE ] ".to_string()
    };

    let block = Block::default()
        .title(Span::styled(
            scroll_indicator,
            Style::default().fg(GREEN).bold(),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(BORDER));

    let list = list.block(block);
    frame.render_widget(list, area);
}

fn render_transfer_log(frame: &mut Frame, area: Rect, app: &AppState) {
    let inner_height = area.height.saturating_sub(2) as usize;

    let items: Vec<ListItem> = app
        .log_entries
        .iter()
        .map(|entry| {
            let (msg_color, prefix) = match entry.level {
                LogLevel::Info => (TEXT_PRIMARY, ""),
                LogLevel::Success => (GREEN, "OK: "),
                LogLevel::Warning => (WARNING_AMBER, "WARN: "),
                LogLevel::Error => (ERROR_RED, "ERR: "),
            };

            let line = Line::from(vec![
                Span::styled(
                    format!("  [{}] ", entry.timestamp),
                    Style::default().fg(MUTED),
                ),
                Span::styled(prefix, Style::default().fg(msg_color).bold()),
                Span::styled(&entry.message, Style::default().fg(msg_color)),
            ]);

            ListItem::new(line)
        })
        .collect();

    // Apply scroll offset for log
    let total = items.len();
    let start = if total > inner_height {
        let max_start = total.saturating_sub(inner_height);
        // log_scroll == total-1 means "bottom" (auto-scroll)
        // Moving up from bottom: log_scroll < total - 1
        if app.log_scroll >= max_start {
            max_start
        } else {
            app.log_scroll
        }
    } else {
        0
    };
    let visible: Vec<ListItem> = items.into_iter().skip(start).take(inner_height).collect();

    let list = if visible.is_empty() {
        List::new(vec![ListItem::new(Line::from(vec![Span::styled(
            "  // Waiting for events...",
            Style::default().fg(TEXT_DIM),
        )]))])
    } else {
        List::new(visible)
    };

    let log_title = if total > inner_height {
        format!(" [ SYSTEM LOG ] ({}/{}) ", start + 1, total)
    } else {
        " [ SYSTEM LOG ] ".to_string()
    };

    let block = Block::default()
        .title(Span::styled(
            log_title,
            Style::default().fg(GREEN).bold(),
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
            Constraint::Length(3), // Progress bar + ETA
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

    // Calculate ETA
    let eta_str = if app.current_speed > 0.0 && app.total_bytes_expected > app.total_bytes_transferred {
        let remaining_bytes = app.total_bytes_expected - app.total_bytes_transferred;
        let remaining_secs = remaining_bytes as f64 / app.current_speed;
        if remaining_secs < 60.0 {
            format!("  ETA {}s", remaining_secs.ceil() as u64)
        } else if remaining_secs < 3600.0 {
            let mins = (remaining_secs / 60.0).floor() as u64;
            let secs = (remaining_secs % 60.0).ceil() as u64;
            format!("  ETA {}m {}s", mins, secs)
        } else {
            let hours = (remaining_secs / 3600.0).floor() as u64;
            let mins = ((remaining_secs % 3600.0) / 60.0).ceil() as u64;
            format!("  ETA {}h {}m", hours, mins)
        }
    } else if percentage >= 100 {
        "  DONE".to_string()
    } else {
        String::new()
    };

    // Elapsed time
    let elapsed = app.session_start.elapsed().as_secs();
    let elapsed_str = if elapsed < 60 {
        format!("{}s", elapsed)
    } else if elapsed < 3600 {
        format!("{}m {}s", elapsed / 60, elapsed % 60)
    } else {
        format!("{}h {}m", elapsed / 3600, (elapsed % 3600) / 60)
    };

    let label = format!(
        "{}%  {}  ({} / {})  [{}]{}",
        percentage, speed_str, transferred, total, elapsed_str, eta_str
    );

    // File count stats
    let completed = app.file_queue.iter().filter(|f| f.status == TransferStatus::Completed).count();
    let total_files = app.file_queue.len();
    let files_str = if total_files > 0 {
        format!(" Files: {}/{} ", completed, total_files)
    } else {
        String::new()
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if percentage >= 100 { GREEN } else { BORDER }))
                .title(Span::styled(files_str, Style::default().fg(GREEN_DIM)))
        )
        .gauge_style(
            Style::default()
                .fg(if percentage >= 100 { GREEN_BRIGHT } else { GREEN })
                .bg(DARK_GRAY)
                .add_modifier(Modifier::BOLD),
        )
        .percent(percentage)
        .label(Span::styled(label, Style::default().fg(TEXT_PRIMARY).bold()));

    frame.render_widget(gauge, area);
}

fn render_sparkline(frame: &mut Frame, area: Rect, app: &AppState) {
    let max_speed = app.speed_history.iter().copied().max().unwrap_or(1).max(1);

    let sparkline = Sparkline::default()
        .block(Block::default().title(Span::styled(
            " Speed ",
            Style::default().fg(TEXT_DIM),
        )))
        .data(&app.speed_history)
        .max(max_speed)
        .style(Style::default().fg(GREEN));

    frame.render_widget(sparkline, area);
}

fn render_keybinds(frame: &mut Frame, area: Rect, _app: &AppState) {
    let keybinds = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled("[CTRL+C]", Style::default().fg(GREEN).bold()),
        Span::styled(" abort  ", Style::default().fg(TEXT_DIM)),
        Span::styled("[Q]", Style::default().fg(GREEN).bold()),
        Span::styled(" exit  ", Style::default().fg(TEXT_DIM)),
        Span::styled("[↑↓/jk]", Style::default().fg(GREEN).bold()),
        Span::styled(" scroll queue  ", Style::default().fg(TEXT_DIM)),
        Span::styled("[PgUp/Dn]", Style::default().fg(GREEN).bold()),
        Span::styled(" scroll log  ", Style::default().fg(TEXT_DIM)),
        Span::styled("[H]", Style::default().fg(GREEN).bold()),
        Span::styled(" home  ", Style::default().fg(TEXT_DIM)),
        Span::styled("[E]", Style::default().fg(GREEN).bold()),
        Span::styled(" end", Style::default().fg(TEXT_DIM)),
    ]);

    let paragraph = Paragraph::new(keybinds).block(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(BORDER)),
    );

    frame.render_widget(paragraph, area);
}
