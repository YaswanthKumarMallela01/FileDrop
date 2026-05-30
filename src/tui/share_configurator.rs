//! Interactive configuration TUI for ephemeral file sharing.
//!
//! Allows the user to browse files and folders, configure link expiry,
//! enable optional 4-digit PIN protection, set a download limit (once vs unlimited),
//! and confirm to generate a QR code and start the share server.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::hotspot::ConnectionMode;

// ── Hacker Theme Colors ─────────────────────────────────────────────────────
const GREEN: Color = Color::Rgb(0, 255, 65);
const GREEN_BRIGHT: Color = Color::Rgb(100, 255, 130);
const WARNING_AMBER: Color = Color::Rgb(255, 176, 0);
const TEXT_PRIMARY: Color = Color::Rgb(180, 180, 180);
const TEXT_DIM: Color = Color::Rgb(100, 100, 100);
const MUTED: Color = Color::Rgb(85, 85, 85);
const BORDER_ACTIVE: Color = GREEN;
const BORDER_INACTIVE: Color = Color::Rgb(40, 40, 40);
const HIGHLIGHT_BG: Color = Color::Rgb(20, 40, 20);

/// Resolved configuration returned by the interactive configurator
#[derive(Debug, Clone)]
pub struct ShareConfig {
    /// Canonical paths of all selected files and folders
    pub selected_paths: Vec<PathBuf>,
    /// Link expiration duration
    pub expires: Duration,
    /// Whether the link expires after the first download
    pub once: bool,
    /// Optional 4-digit PIN required before download
    pub pin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FocusPane {
    FileSelector,
    Configurator,
}

#[derive(Debug, Clone)]
struct ShareFileEntry {
    name: String,
    is_dir: bool,
    size: u64,
    path: PathBuf,
}

struct ConfigState {
    expiry_options: Vec<(&'static str, Duration)>,
    expiry_index: usize,
    pin_enabled: bool,
    pin_code: String,
    once: bool,
    config_cursor: usize, // 0 = Expiry, 1 = PIN Enabled, 2 = PIN Code, 3 = Limit, 4 = Generate button
}

impl ConfigState {
    fn new() -> Self {
        Self {
            expiry_options: vec![
                ("5 minutes", Duration::from_secs(5 * 60)),
                ("10 minutes", Duration::from_secs(10 * 60)),
                ("15 minutes", Duration::from_secs(15 * 60)),
                ("30 minutes", Duration::from_secs(30 * 60)),
                ("1 hour", Duration::from_secs(60 * 60)),
                ("2 hours", Duration::from_secs(2 * 60 * 60)),
            ],
            expiry_index: 2, // 15 minutes default
            pin_enabled: false,
            pin_code: String::new(),
            once: true,
            config_cursor: 0,
        }
    }

    fn current_duration(&self) -> Duration {
        self.expiry_options[self.expiry_index].1
    }

    fn current_duration_str(&self) -> &'static str {
        self.expiry_options[self.expiry_index].0
    }
}

/// Run the interactive Share Configurator alternate screen TUI.
pub async fn run_share_configurator(
    start_path: PathBuf,
    connection_mode: ConnectionMode,
) -> Result<Option<ShareConfig>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Resolve directory and optional starting pre-selection
    let (mut current_dir, pre_select_name) = if start_path.is_file() {
        (
            start_path.parent().unwrap_or(&start_path).to_path_buf(),
            Some(start_path.file_name().unwrap_or_default().to_string_lossy().to_string()),
        )
    } else {
        (
            if start_path.exists() {
                start_path.clone()
            } else {
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
            },
            None,
        )
    };

    // Canonicalize path to be safe
    if let Ok(canon) = tokio::fs::canonicalize(&current_dir).await {
        current_dir = canon;
    }

    let mut entries = load_entries(&current_dir);
    let mut cursor: usize = 0;
    let mut selected_indices: HashSet<usize> = HashSet::new();

    // If starting with a file pre-selection, find and select it!
    if let Some(ref target_name) = pre_select_name {
        if let Some(pos) = entries.iter().position(|e| e.name == *target_name) {
            cursor = pos;
            selected_indices.insert(pos);
        }
    }

    let mut focus = FocusPane::FileSelector;
    let mut config_state = ConfigState::new();
    let mut result = None;

    // Drain any pending input events first
    tokio::time::sleep(Duration::from_millis(150)).await;
    while event::poll(Duration::from_millis(0))? {
        let _ = event::read();
    }

    loop {
        terminal.draw(|frame| {
            render_configurator(
                frame,
                &current_dir,
                &entries,
                cursor,
                &selected_indices,
                &focus,
                &config_state,
                &connection_mode,
            );
        })?;

        let has_event = tokio::task::spawn_blocking(|| {
            event::poll(Duration::from_millis(100))
        }).await??;

        if !has_event {
            continue;
        }

        let evt = tokio::task::spawn_blocking(event::read).await??;
        if let Event::Key(key) = evt {
            if key.kind == KeyEventKind::Release {
                continue;
            }

            match key.code {
                // Abort / Quit
                KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                    break;
                }

                // Global generation key trigger (requires at least one file/folder selected)
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    if !selected_indices.is_empty() {
                        let selected_paths = selected_indices
                            .iter()
                            .filter_map(|&idx| entries.get(idx))
                            .map(|e| e.path.clone())
                            .collect();
                        
                        let pin = if config_state.pin_enabled && config_state.pin_code.len() == 4 {
                            Some(config_state.pin_code.clone())
                        } else {
                            None
                        };

                        result = Some(ShareConfig {
                            selected_paths,
                            expires: config_state.current_duration(),
                            once: config_state.once,
                            pin,
                        });
                        break;
                    }
                }

                // Switch Focus
                KeyCode::Tab => {
                    focus = match focus {
                        FocusPane::FileSelector => FocusPane::Configurator,
                        FocusPane::Configurator => FocusPane::FileSelector,
                    };
                }

                // Up / j
                KeyCode::Up | KeyCode::Char('j') | KeyCode::Char('J') => {
                    match focus {
                        FocusPane::FileSelector => {
                            if cursor > 0 {
                                cursor -= 1;
                            } else if !entries.is_empty() {
                                cursor = entries.len() - 1;
                            }
                        }
                        FocusPane::Configurator => {
                            if config_state.config_cursor > 0 {
                                config_state.config_cursor -= 1;
                            } else {
                                config_state.config_cursor = 4;
                            }
                        }
                    }
                }

                // Down / k
                KeyCode::Down | KeyCode::Char('k') | KeyCode::Char('K') => {
                    match focus {
                        FocusPane::FileSelector => {
                            if !entries.is_empty() {
                                if cursor < entries.len() - 1 {
                                    cursor += 1;
                                } else {
                                    cursor = 0;
                                }
                            }
                        }
                        FocusPane::Configurator => {
                            if config_state.config_cursor < 4 {
                                config_state.config_cursor += 1;
                            } else {
                                config_state.config_cursor = 0;
                            }
                        }
                    }
                }

                // Space / toggle selection (only in FileSelector)
                KeyCode::Char(' ') => {
                    if focus == FocusPane::FileSelector && !entries.is_empty() {
                        let entry = &entries[cursor];
                        if entry.name != ".." {
                            if selected_indices.contains(&cursor) {
                                selected_indices.remove(&cursor);
                            } else {
                                selected_indices.insert(cursor);
                            }
                        }
                    }
                }

                // Enter / Action
                KeyCode::Enter => {
                    match focus {
                        FocusPane::FileSelector => {
                            if !entries.is_empty() {
                                let entry = entries[cursor].clone();
                                if entry.is_dir {
                                    // Enter directory
                                    current_dir = entry.path;
                                    entries = load_entries(&current_dir);
                                    cursor = 0;
                                    selected_indices.clear();
                                } else {
                                    // Toggle selection
                                    if selected_indices.contains(&cursor) {
                                        selected_indices.remove(&cursor);
                                    } else {
                                        selected_indices.insert(cursor);
                                    }
                                }
                            }
                        }
                        FocusPane::Configurator => {
                            if config_state.config_cursor == 4 {
                                // Trigger generate share link (same as 'S')
                                if !selected_indices.is_empty() {
                                    let selected_paths = selected_indices
                                        .iter()
                                        .filter_map(|&idx| entries.get(idx))
                                        .map(|e| e.path.clone())
                                        .collect();
                                    
                                    let pin = if config_state.pin_enabled && config_state.pin_code.len() == 4 {
                                        Some(config_state.pin_code.clone())
                                    } else {
                                        None
                                    };

                                    result = Some(ShareConfig {
                                        selected_paths,
                                        expires: config_state.current_duration(),
                                        once: config_state.once,
                                        pin,
                                    });
                                    break;
                                }
                            }
                        }
                    }
                }

                // Backspace (goes up one directory level)
                KeyCode::Backspace => {
                    if focus == FocusPane::FileSelector {
                        if let Some(parent) = current_dir.parent() {
                            current_dir = parent.to_path_buf();
                            entries = load_entries(&current_dir);
                            cursor = 0;
                            selected_indices.clear();
                        }
                    } else if focus == FocusPane::Configurator && config_state.config_cursor == 2 {
                        // Delete PIN digit
                        config_state.pin_code.pop();
                    }
                }

                // Left Arrow
                KeyCode::Left => {
                    if focus == FocusPane::Configurator {
                        match config_state.config_cursor {
                            0 => {
                                // Expiry
                                if config_state.expiry_index > 0 {
                                    config_state.expiry_index -= 1;
                                } else {
                                    config_state.expiry_index = config_state.expiry_options.len() - 1;
                                }
                            }
                            1 => {
                                // PIN Toggle
                                config_state.pin_enabled = !config_state.pin_enabled;
                            }
                            3 => {
                                // Limit
                                config_state.once = !config_state.once;
                            }
                            _ => {}
                        }
                    }
                }

                // Right Arrow
                KeyCode::Right => {
                    if focus == FocusPane::Configurator {
                        match config_state.config_cursor {
                            0 => {
                                // Expiry
                                if config_state.expiry_index < config_state.expiry_options.len() - 1 {
                                    config_state.expiry_index += 1;
                                } else {
                                    config_state.expiry_index = 0;
                                }
                            }
                            1 => {
                                // PIN Toggle
                                config_state.pin_enabled = !config_state.pin_enabled;
                            }
                            3 => {
                                // Limit
                                config_state.once = !config_state.once;
                            }
                            _ => {}
                        }
                    }
                }

                // PIN protection characters (numbers 0-9)
                KeyCode::Char(c) if c.is_ascii_digit() => {
                    if focus == FocusPane::Configurator && config_state.config_cursor == 2 && config_state.pin_enabled {
                        if config_state.pin_code.len() < 4 {
                            config_state.pin_code.push(c);
                        }
                    }
                }

                _ => {}
            }
        }
    }

    // Restore terminal alternate screen
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(result)
}

fn load_entries(dir: &Path) -> Vec<ShareFileEntry> {
    let mut entries = Vec::new();

    // 1. Add ".." navigation if parent exists
    if let Some(parent) = dir.parent() {
        entries.push(ShareFileEntry {
            name: "..".to_string(),
            is_dir: true,
            size: 0,
            path: parent.to_path_buf(),
        });
    }

    // 2. Read current directory entries
    if let Ok(read_dir) = std::fs::read_dir(dir) {
        let mut child_entries = Vec::new();
        for entry in read_dir.filter_map(|e| e.ok()) {
            if let Ok(meta) = entry.metadata() {
                let name = entry.file_name().to_string_lossy().to_string();
                // Skip hidden files
                if name.starts_with('.') {
                    continue;
                }
                child_entries.push(ShareFileEntry {
                    name,
                    is_dir: meta.is_dir(),
                    size: if meta.is_file() { meta.len() } else { 0 },
                    path: entry.path(),
                });
            }
        }

        // Sort directories first, then files
        child_entries.sort_by(|a, b| {
            b.is_dir.cmp(&a.is_dir).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        entries.extend(child_entries);
    }

    entries
}

fn render_configurator(
    frame: &mut Frame,
    current_dir: &Path,
    entries: &[ShareFileEntry],
    cursor: usize,
    selected_indices: &HashSet<usize>,
    focus: &FocusPane,
    config: &ConfigState,
    connection_mode: &ConnectionMode,
) {
    let area = frame.area();

    // Main layout: Header block, Middle split-pane, Footer help binds
    let main_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(5),    // Split Middle panes
            Constraint::Length(4), // Footer / Hotkeys
        ])
        .split(area);

    // ── Render Header ──
    let mode_str = connection_mode.as_str().to_uppercase();
    let header_spans = vec![
        Span::styled("  [", Style::default().fg(MUTED)),
        Span::styled("FILEDROP", Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
        Span::styled("] ", Style::default().fg(MUTED)),
        Span::styled("v0.2.0 ", Style::default().fg(TEXT_DIM)),
        Span::styled(" :: ", Style::default().fg(MUTED)),
        Span::styled("EPHEMERAL SHARE CONFIGURATOR", Style::default().fg(GREEN_BRIGHT)),
        Span::styled(" :: ", Style::default().fg(MUTED)),
        Span::styled(format!("MODE: {}", mode_str), Style::default().fg(WARNING_AMBER).add_modifier(Modifier::BOLD)),
    ];
    let header = Paragraph::new(Line::from(header_spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(GREEN)),
    );
    frame.render_widget(header, main_layout[0]);

    // ── Render Middle Panes ──
    let middle_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // Left: File Selector
            Constraint::Percentage(45), // Right: Configurator settings
        ])
        .split(main_layout[1]);

    // Left: File Selector
    let file_explorer_active = *focus == FocusPane::FileSelector;
    let file_border_style = if file_explorer_active {
        Style::default().fg(BORDER_ACTIVE)
    } else {
        Style::default().fg(BORDER_INACTIVE)
    };

    let title_prefix = if file_explorer_active { "► " } else { "  " };
    let file_title = format!("{}[ BROWSE FILES: {} ]", title_prefix, current_dir.display());

    let file_items: Vec<ListItem> = entries
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let is_highlighted = i == cursor && file_explorer_active;
            let is_selected = selected_indices.contains(&i);

            let prefix = if entry.name == ".." {
                "   "
            } else if is_selected {
                "[✓] "
            } else {
                "[ ] "
            };

            let icon = if entry.is_dir { "📁 " } else { "📄 " };
            let size_str = if entry.name == ".." {
                "".to_string()
            } else if entry.is_dir {
                "[Dir]".to_string()
            } else {
                format_bytes(entry.size)
            };

            let name_span = Span::styled(
                format!("{}{}{}", prefix, icon, entry.name),
                if is_highlighted {
                    Style::default().fg(GREEN_BRIGHT).bg(HIGHLIGHT_BG).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(GREEN).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT_PRIMARY)
                },
            );

            let size_span = Span::styled(
                size_str,
                if is_highlighted {
                    Style::default().fg(GREEN_BRIGHT).bg(HIGHLIGHT_BG)
                } else {
                    Style::default().fg(TEXT_DIM)
                },
            );

            // Split name and size to ends of list item
            let layout_width = middle_layout[0].width.saturating_sub(4) as usize;
            let name_len = entry.name.len() + 10;
            let padding_len = layout_width.saturating_sub(name_len).saturating_sub(size_span.content.len());
            let padding = " ".repeat(padding_len);

            ListItem::new(Line::from(vec![
                name_span,
                Span::raw(padding),
                size_span,
            ]))
        })
        .collect();

    let file_list = List::new(file_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(file_border_style)
            .title(Span::styled(file_title, Style::default().fg(GREEN).bold())),
    );
    frame.render_widget(file_list, middle_layout[0]);

    // Right: Share Configurator
    let configurator_active = *focus == FocusPane::Configurator;
    let config_border_style = if configurator_active {
        Style::default().fg(BORDER_ACTIVE)
    } else {
        Style::default().fg(BORDER_INACTIVE)
    };

    let config_title_prefix = if configurator_active { "► " } else { "  " };
    let config_title = format!("{}[ CONFIGURATION ]", config_title_prefix);

    let config_block = Block::default()
        .borders(Borders::ALL)
        .border_style(config_border_style)
        .title(Span::styled(config_title, Style::default().fg(GREEN).bold()));

    let inner_area = config_block.inner(middle_layout[1]);
    frame.render_widget(config_block, middle_layout[1]);

    // Render configuration settings fields inside inner area
    let fields_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Spacer
            Constraint::Length(2), // Expiry
            Constraint::Length(2), // PIN Enable
            Constraint::Length(2), // PIN Code (editable)
            Constraint::Length(2), // Download Limit (once vs unlimited)
            Constraint::Length(1), // Spacer
            Constraint::Length(3), // Action Button: [ ▶ GENERATE SHARE LINK ]
            Constraint::Min(0),    // Rest spacer
        ])
        .split(inner_area);

    // 1. Expiry Duration Option
    let is_field0 = config.config_cursor == 0 && configurator_active;
    let field0_spans = vec![
        Span::styled("  Link Expiry : ", Style::default().fg(TEXT_PRIMARY)),
        Span::styled(
            format!("◄  {}  ►", config.current_duration_str()),
            if is_field0 {
                Style::default().fg(GREEN_BRIGHT).bg(HIGHLIGHT_BG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(GREEN)
            },
        ),
    ];
    frame.render_widget(Paragraph::new(Line::from(field0_spans)), fields_layout[1]);

    // 2. PIN Protection Enable Option
    let is_field1 = config.config_cursor == 1 && configurator_active;
    let pin_toggle_str = if config.pin_enabled { "◄  ENABLED   ►" } else { "◄  DISABLED  ►" };
    let field1_spans = vec![
        Span::styled("  PIN Security: ", Style::default().fg(TEXT_PRIMARY)),
        Span::styled(
            pin_toggle_str,
            if is_field1 {
                Style::default().fg(GREEN_BRIGHT).bg(HIGHLIGHT_BG).add_modifier(Modifier::BOLD)
            } else if config.pin_enabled {
                Style::default().fg(WARNING_AMBER).bold()
            } else {
                Style::default().fg(TEXT_DIM)
            },
        ),
    ];
    frame.render_widget(Paragraph::new(Line::from(field1_spans)), fields_layout[2]);

    // 3. PIN Code (editable if PIN is ON)
    let is_field2 = config.config_cursor == 2 && configurator_active;
    let pin_display = if !config.pin_enabled {
        "[Security Disabled]".to_string()
    } else if config.pin_code.is_empty() {
        "[Type 4-digit PIN]".to_string()
    } else {
        format!("  {}  ", config.pin_code)
    };
    let field2_style = if !config.pin_enabled {
        Style::default().fg(MUTED)
    } else if is_field2 {
        Style::default().fg(GREEN_BRIGHT).bg(HIGHLIGHT_BG).add_modifier(Modifier::BOLD)
    } else if config.pin_code.len() == 4 {
        Style::default().fg(GREEN).bold()
    } else {
        Style::default().fg(WARNING_AMBER)
    };
    let field2_spans = vec![
        Span::styled("  4-Digit PIN : ", Style::default().fg(TEXT_PRIMARY)),
        Span::styled(pin_display, field2_style),
    ];
    frame.render_widget(Paragraph::new(Line::from(field2_spans)), fields_layout[3]);

    // 4. Download Limit
    let is_field3 = config.config_cursor == 3 && configurator_active;
    let limit_str = if config.once { "◄  ONCE (Single)  ►" } else { "◄  UNLIMITED (Multi)  ►" };
    let field3_spans = vec![
        Span::styled("  Download Max: ", Style::default().fg(TEXT_PRIMARY)),
        Span::styled(
            limit_str,
            if is_field3 {
                Style::default().fg(GREEN_BRIGHT).bg(HIGHLIGHT_BG).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(GREEN)
            },
        ),
    ];
    frame.render_widget(Paragraph::new(Line::from(field3_spans)), fields_layout[4]);

    // 5. Generate Link Button
    let is_field4 = config.config_cursor == 4 && configurator_active;
    let btn_style = if is_field4 {
        Style::default().fg(Color::Black).bg(GREEN_BRIGHT).add_modifier(Modifier::BOLD)
    } else if selected_indices.is_empty() {
        Style::default().fg(MUTED).dim()
    } else {
        Style::default().fg(Color::Black).bg(GREEN).bold()
    };
    let btn_text = if selected_indices.is_empty() {
        "   [ SELECT FILES FIRST TO GENERATE ]   "
    } else {
        "   [ ▶ GENERATE SHARE LINK (S) ]   "
    };

    let action_btn = Paragraph::new(Line::from(vec![Span::raw("  "), Span::styled(btn_text, btn_style)]));
    frame.render_widget(action_btn, fields_layout[6]);

    // ── Render Footer ──
    let selected_count = selected_indices.len();
    let selected_size = selected_indices
        .iter()
        .filter_map(|&idx| entries.get(idx))
        .map(|e| e.size)
        .sum();

    let footer_text = format!(
        " Selected: {} items · Total Size: {}\n Navigate: ↑/↓   Toggle File: Enter/Space   Switch Pane: Tab\n Change Value: ←/→   Type PIN: 0-9 / Backspace\n Confirm: S (Global)   Cancel / Quit: Q / Esc",
        selected_count,
        format_bytes(selected_size)
    );

    let footer = Paragraph::new(footer_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(BORDER_INACTIVE))
            .title(Span::styled(" [ HELPBINDS ] ", Style::default().fg(TEXT_DIM))),
    );
    frame.render_widget(footer, main_layout[2]);
}

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
