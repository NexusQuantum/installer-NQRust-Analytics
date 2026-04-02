use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::app::keycloak_form_data::{FocusState, KeycloakFormData, ROLES, SCHEMES};
use crate::ui::{get_orange_accent, get_orange_color};

pub struct KeycloakConfigView<'a> {
    pub form_data: &'a KeycloakFormData,
}

pub fn render_keycloak_config(frame: &mut Frame, view: &KeycloakConfigView<'_>) {
    let area = frame.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(2)
        .constraints([
            Constraint::Length(3), // title
            Constraint::Min(10),   // form
            Constraint::Length(4), // error + help
            Constraint::Length(3), // buttons
        ])
        .split(area);

    let title = Paragraph::new("🔐 NQRust Identity SSO (Optional)")
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

    let data = view.form_data;
    let mut lines: Vec<Line> = vec![Line::from("")];

    // --- Enable toggle ---
    let enable_focused = matches!(data.focus_state, FocusState::EnableToggle);
    let enable_style = if enable_focused {
        Style::default()
            .fg(Color::Black)
            .bg(get_orange_color())
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    };
    let enable_value = if data.enabled {
        Span::styled(
            " YES ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Green)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            " NO  ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
    };
    lines.push(Line::from(vec![
        Span::styled("  Enable Identity SSO: ", enable_style),
        enable_value,
        Span::styled("  (Space to toggle)", Style::default().fg(Color::DarkGray)),
    ]));
    lines.push(Line::from(""));

    if data.enabled {
        // --- Scheme selector (http / https) ---
        let scheme_focused = matches!(data.focus_state, FocusState::SchemeSelect);
        let scheme_label_style = if scheme_focused {
            Style::default()
                .fg(Color::Black)
                .bg(get_orange_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let mut scheme_spans = vec![Span::styled("  Protocol: ", scheme_label_style)];
        for (i, s) in SCHEMES.iter().enumerate() {
            let selected = i == data.scheme_index;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if scheme_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            scheme_spans.push(Span::styled(format!(" {} ", s), style));
        }
        scheme_spans.push(Span::styled(
            "  (← → to change)",
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::from(scheme_spans));
        lines.push(Line::from(""));

        // --- Text fields (host, port, realm, client_id, client_secret) ---
        for i in 0..KeycloakFormData::total_text_fields() {
            let is_focused = matches!(&data.focus_state, FocusState::Field(idx) if *idx == i);
            let field_style = if is_focused {
                Style::default()
                    .fg(Color::Black)
                    .bg(get_orange_color())
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let value = match i {
                0 => &data.host,
                1 => &data.port,
                2 => &data.realm,
                3 => &data.client_id,
                4 => &data.client_secret,
                _ => unreachable!(),
            };

            let display_value = if i == 4 && !value.is_empty() {
                // mask client secret
                if value.len() > 8 {
                    format!("{}...{}", &value[..4], &value[value.len() - 4..])
                } else {
                    "*".repeat(value.len())
                }
            } else if value.is_empty() {
                "<empty>".to_string()
            } else {
                value.clone()
            };

            let label = KeycloakFormData::field_name(i);
            lines.push(Line::from(vec![
                Span::styled(format!("  {}: ", label), field_style),
                Span::styled(
                    display_value,
                    if is_focused {
                        Style::default().fg(Color::Black).bg(get_orange_color())
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ),
            ]));

            // After port field, show derived URLs
            if i == 1 && !data.host.trim().is_empty() {
                let public = data.public_url();
                let keycloak = data.keycloak_url();
                lines.push(Line::from(vec![
                    Span::styled("    → Public URL: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(public, Style::default().fg(Color::DarkGray)),
                ]));
                if keycloak != data.public_url() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "    → Identity URL (internal): ",
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(keycloak, Style::default().fg(Color::DarkGray)),
                    ]));
                }
            }
            lines.push(Line::from(""));
        }

        // --- Default Role selector ---
        let role_focused = matches!(data.focus_state, FocusState::RoleSelect);
        let role_label_style = if role_focused {
            Style::default()
                .fg(Color::Black)
                .bg(get_orange_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let mut role_spans = vec![Span::styled("  Default Role: ", role_label_style)];
        for (i, role) in ROLES.iter().enumerate() {
            let selected = i == data.default_role_index;
            let style = if selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else if role_focused {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            role_spans.push(Span::styled(format!(" {} ", role), style));
        }
        role_spans.push(Span::styled(
            "  (← → to change)",
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::from(role_spans));
        lines.push(Line::from(""));

        // --- Auto Register toggle ---
        let ar_focused = matches!(data.focus_state, FocusState::AutoRegisterToggle);
        let ar_label_style = if ar_focused {
            Style::default()
                .fg(Color::Black)
                .bg(get_orange_color())
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let ar_value = if data.auto_register {
            Span::styled(
                " YES ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                " NO  ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
        };
        lines.push(Line::from(vec![
            Span::styled("  Auto-register new SSO users: ", ar_label_style),
            ar_value,
            Span::styled("  (Space to toggle)", Style::default().fg(Color::DarkGray)),
        ]));
    }

    let form = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(get_orange_accent()))
                .title("Identity SSO Configuration")
                .title_style(
                    Style::default()
                        .fg(get_orange_color())
                        .add_modifier(Modifier::BOLD),
                ),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(form, chunks[1]);

    // --- Error / hint ---
    let hint_text = if !data.error_message.is_empty() {
        Line::from(Span::styled(
            &data.error_message,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ))
    } else {
        Line::from(Span::styled(
            "Tab/↑↓ to navigate • Space to toggle • ← → for role • Enter to edit field • Ctrl+S to save • Esc to skip",
            Style::default().fg(Color::DarkGray),
        ))
    };
    let hint = Paragraph::new(vec![Line::from(""), hint_text])
        .block(Block::default().borders(Borders::NONE))
        .centered();
    frame.render_widget(hint, chunks[2]);

    // --- Buttons ---
    let save_focused = matches!(data.focus_state, FocusState::SaveButton);
    let cancel_focused = matches!(data.focus_state, FocusState::CancelButton);

    let save_style = if save_focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Green)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green)
    };
    let cancel_style = if cancel_focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Red)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Red)
    };

    let button_line = Line::from(vec![
        Span::styled("  [ Save ]  ", save_style),
        Span::raw("    "),
        Span::styled("  [ Skip ]  ", cancel_style),
    ]);
    let buttons = Paragraph::new(vec![button_line])
        .block(Block::default().borders(Borders::NONE))
        .centered();
    frame.render_widget(buttons, chunks[3]);
}
