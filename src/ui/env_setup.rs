use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};

use crate::app::form_data::{FocusState, FormData};
use crate::ui::{get_orange_accent, get_orange_color};

pub struct EnvSetupView<'a> {
    pub form_data: &'a FormData,
}

pub fn render_env_setup(frame: &mut Frame, view: &EnvSetupView<'_>) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(3),
        ])
        .split(area);

    let data = view.form_data;

    let title_text = if !data.selected_provider.is_empty() {
        format!("🔧 Generate .env File - {}", data.get_api_key_name())
    } else {
        "🔧 Generate .env File".to_string()
    };

    let title = Paragraph::new(title_text)
        .style(
            Style::default()
                .fg(get_orange_color())
                .add_modifier(Modifier::BOLD),
        )
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(get_orange_accent())),
        )
        .centered();
    frame.render_widget(title, chunks[0]);

    let mut form_lines = vec![];

    let needs_openai = data.needs_openai_embedding();

    // Show auto-detected NEXTAUTH_URL info
    let local_ip = crate::utils::detect_local_ip();
    form_lines.push(Line::from(vec![
        Span::styled("ℹ  Analytics UI URL: ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("http://{}:3000", local_ip),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" (auto-detected)", Style::default().fg(Color::DarkGray)),
    ]));
    form_lines.push(Line::from(""));

    // Helper closure: render a masked API key field
    let make_key_field = |label: &str, value: &str, focused: bool| -> Vec<Line<'static>> {
        let style = if focused {
            Style::default()
                .fg(Color::Black)
                .bg(get_orange_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let cursor = if focused { "▶" } else { " " };
        let display = if value.is_empty() {
            "<empty>".to_string()
        } else if value.len() > 8 {
            format!("{}...{}", &value[..4], &value[value.len() - 4..])
        } else {
            "*".repeat(value.len())
        };
        vec![
            Line::from(vec![
                Span::styled(cursor.to_string(), style),
                Span::raw(" "),
                Span::styled(label.to_string(), style),
                Span::raw(": "),
                Span::styled(display, style),
            ]),
            Line::from(""),
        ]
    };

    // Field 0: Provider API Key
    let api_key_name = data.get_api_key_name();
    form_lines.extend(make_key_field(
        &format!("{} API Key", api_key_name),
        &data.api_key,
        matches!(&data.focus_state, FocusState::Field(0)),
    ));

    // Field 1: OpenAI API Key (if needed for embedding)
    if needs_openai {
        form_lines.extend(make_key_field(
            "OpenAI API Key (embedding)",
            &data.openai_api_key,
            matches!(&data.focus_state, FocusState::Field(1)),
        ));
        form_lines.push(Line::from(Span::styled(
            "ℹ️  This provider uses OpenAI embedding model",
            Style::default().fg(Color::Yellow),
        )));
        form_lines.push(Line::from(""));
    }

    if data.selected_provider == "lm_studio" || data.selected_provider == "ollama" {
        form_lines.push(Line::from(Span::styled(
            "ℹ️  No API key required for local services",
            Style::default().fg(Color::Yellow),
        )));
        form_lines.push(Line::from(""));
    }

    if !data.error_message.is_empty() {
        form_lines.push(Line::from(Span::styled(
            &data.error_message,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        form_lines.push(Line::from(""));
    }

    let form = Paragraph::new(form_lines).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(get_orange_accent()))
            .title("API Keys")
            .title_style(
                Style::default()
                    .fg(get_orange_color())
                    .add_modifier(Modifier::BOLD),
            ),
    );
    frame.render_widget(form, chunks[1]);

    // Buttons
    let save_focused = matches!(&data.focus_state, FocusState::SaveButton);
    let cancel_focused = matches!(&data.focus_state, FocusState::CancelButton);

    let save_style = if save_focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Green)
            .add_modifier(Modifier::BOLD)
    };

    let cancel_style = if cancel_focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    };

    let button_line = Line::from(vec![
        Span::raw("  "),
        Span::styled(" Save ", save_style),
        Span::raw("  "),
        Span::styled(" Cancel ", cancel_style),
        Span::raw("  "),
        Span::styled("↑↓ Tab to navigate", Style::default().fg(Color::DarkGray)),
    ]);

    let buttons = Paragraph::new(button_line).centered();
    frame.render_widget(buttons, chunks[2]);
}
