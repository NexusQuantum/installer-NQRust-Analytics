use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::MenuSelection;
use crate::ui::{ASCII_HEADER, get_orange_accent, get_orange_color};

pub struct ConfirmationView<'a> {
    pub env_exists: bool,
    pub config_exists: bool,
    pub menu_selection: &'a MenuSelection,
    pub menu_options: &'a [MenuSelection],
}

pub fn render_confirmation(frame: &mut Frame, view: &ConfirmationView<'_>) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(5), // ASCII header (smaller - 6 lines but compact)
            Constraint::Min(10),
            Constraint::Length(6),
            Constraint::Length(2),
        ])
        .split(area);

    // Render ASCII header in orange
    let header_lines: Vec<Line> = ASCII_HEADER
        .trim()
        .lines()
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default()
                    .fg(get_orange_color())
                    .add_modifier(Modifier::BOLD),
            ))
        })
        .collect();

    let header = Paragraph::new(header_lines)
        .block(Block::default().borders(Borders::NONE))
        .centered();
    frame.render_widget(header, chunks[0]);

    let all_files_exist = view.env_exists && view.config_exists;

    let mut content_lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Configuration Files:",
            Style::default().fg(if all_files_exist {
                Color::Green
            } else {
                Color::Yellow
            }),
        )),
        Line::from(""),
    ];

    content_lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            if view.env_exists { "✓" } else { "✗" },
            Style::default().fg(if view.env_exists {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Span::raw(" .env"),
        if !view.env_exists {
            Span::styled(" (missing)", Style::default().fg(Color::Red))
        } else {
            Span::raw("")
        },
    ]));

    content_lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            if view.config_exists { "✓" } else { "✗" },
            Style::default().fg(if view.config_exists {
                Color::Green
            } else {
                Color::Red
            }),
        ),
        Span::raw(" config.yaml"),
        if !view.config_exists {
            Span::styled(" (missing)", Style::default().fg(Color::Red))
        } else {
            Span::raw("")
        },
    ]));

    content_lines.push(Line::from(""));

    if all_files_exist {
        content_lines.push(Line::from(Span::styled(
            "✅ All configuration files ready!",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )));
        content_lines.push(Line::from(""));
        content_lines.push(Line::from("Services to be started:"));
        content_lines.push(Line::from("  • analytics-service"));
        content_lines.push(Line::from("  • qdrant"));
        content_lines.push(Line::from("  • northwind-db (PostgreSQL demo)"));
        content_lines.push(Line::from("  • analytics-ui"));
    } else {
        content_lines.push(Line::from(Span::styled(
            "⚠️  Some configuration files are missing!",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        content_lines.push(Line::from(
            "Please generate the missing files before proceeding.",
        ));
    }

    let content = Paragraph::new(content_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(get_orange_accent()))
                .title("Status")
                .title_style(
                    Style::default()
                        .fg(get_orange_color())
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .centered();
    frame.render_widget(content, chunks[1]);

    let mut menu_lines = vec![Line::from("")];

    for option in view.menu_options {
        let (label, fg_color, highlight_color) = match option {
            MenuSelection::GenerateConfig => (
                "Generate config.yaml",
                get_orange_color(),
                get_orange_color(),
            ),
            MenuSelection::GenerateEnv => ("Generate .env", get_orange_color(), get_orange_color()),
            MenuSelection::CheckUpdates => ("Check for updates", Color::Cyan, Color::Cyan),
            MenuSelection::UpdateToken => (
                "Update GHCR token",
                Color::Yellow,
                Color::Yellow,
            ),
            MenuSelection::Proceed => ("Proceed with installation", Color::Green, Color::Green),
            MenuSelection::Cancel => ("Cancel", Color::Red, Color::Red),
        };

        let style = if option == view.menu_selection {
            Style::default()
                .fg(Color::Black)
                .bg(highlight_color)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(fg_color)
        };

        menu_lines.push(Line::from(Span::styled(format!("  ▶  {}", label), style)));
    }

    let menu = Paragraph::new(menu_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(get_orange_accent()))
                .title("Menu")
                .title_style(
                    Style::default()
                        .fg(get_orange_color())
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .centered();
    frame.render_widget(menu, chunks[2]);

    let help = Paragraph::new("Use ↑↓ to navigate, Enter to select, Esc to cancel")
        .style(Style::default().fg(Color::DarkGray))
        .centered();
    frame.render_widget(help, chunks[3]);
}
