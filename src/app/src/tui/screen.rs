use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};

use crate::cli::Usage;
use crate::composition::{Notice, NoticeLevel};
use crate::tui::{
    ConversationEntry, ConversationView, Entry, KeyBindings, Mode, Overlay, RosterRow, StatusLine,
    UiState, delivery_text,
};

/// Everything one frame draws, gathered by the caller before anything is
/// rendered.
///
/// # Why a snapshot rather than the ports
///
/// The draw path takes data, never collaborators. Two reasons, and the second
/// is the one that matters: a renderer holding query ports would read the
/// roster once for the roster pane and again for the conversation list, and the
/// two could disagree within a single frame — a peer in one pane and not the
/// other. Gathering first also keeps `ratatui` from ever appearing in the same
/// function as a port call.
pub struct ScreenData<'a> {
    pub status: &'a StatusLine,
    pub conversations: &'a [ConversationEntry],
    pub conversation: &'a ConversationView,
    pub roster: &'a [RosterRow],
    pub notices: &'a [Notice],
    pub local_fingerprint: &'a str,
    pub selected_fingerprint: Option<&'a str>,
    /// Which profile directory this instance is running on. With two instances
    /// on one machine (OP-13) the first question is always which one is on
    /// screen, and nothing else on it answers that.
    pub profile: &'a str,
    pub diagnostics: &'a [(String, u64)],
}

/// Draws one frame.
///
/// # The layout
///
/// ```text
/// ┌──────────────────────────────────────────────────────────┐
/// │ isolated │ peer-d75a9801 · 21fe 31df │ broadcast         │  status (S7: says `isolated` plainly)
/// ├───────────────────┬──────────────────────────────────────┤
/// │ conversations     │ conversation, grouped by author      │
/// │ roster            │                                      │
/// │  ✓→ 21fe 31df     │  ── 21fe 31df ──                     │
/// │  ? ⇄ 3d40 17c3    │   → hello            → published     │
/// ├───────────────────┴──────────────────────────────────────┤
/// │ notices — the join account AC3 requires be visible        │
/// ├──────────────────────────────────────────────────────────┤
/// │ > what you are typing                                     │
/// └──────────────────────────────────────────────────────────┘
/// ```
pub fn draw(frame: &mut Frame<'_>, state: &UiState, data: &ScreenData<'_>) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(6),
            Constraint::Length(6),
            Constraint::Length(3),
        ])
        .split(frame.area());

    draw_status(frame, rows[0], data.status);

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(20)])
        .split(rows[1]);

    draw_sidebar(frame, columns[0], state, data);
    draw_conversation(frame, columns[1], data);
    draw_notices(frame, rows[2], data.notices);
    draw_input(frame, rows[3], state);

    draw_overlay(frame, state, data);
}

fn draw_status(frame: &mut Frame<'_>, area: Rect, status: &StatusLine) {
    let style = if status.is_isolated() {
        // Amber, not red: `Isolated` is a normal state (canvas §2.2, S7), and
        // a red status line would tell a user something is broken when a fresh
        // install on a quiet network is behaving exactly as designed.
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::Green)
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(status.text(), style)])),
        area,
    );
}

fn draw_sidebar(frame: &mut Frame<'_>, area: Rect, state: &UiState, data: &ScreenData<'_>) {
    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    let conversations: Vec<ListItem<'_>> = data
        .conversations
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let style = if index == state.selected() {
                Style::default().add_modifier(Modifier::REVERSED)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(Span::styled(entry.label.clone(), style)))
        })
        .collect();

    frame.render_widget(
        List::new(conversations).block(
            Block::default()
                .borders(Borders::ALL)
                .title("conversations"),
        ),
        split[0],
    );

    let roster: Vec<ListItem<'_>> = data
        .roster
        .iter()
        .map(|row| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:>2} ", row.trust_badge()),
                    if row.trust.blocked {
                        Style::default().fg(Color::Red)
                    } else if row.trust.is_verified() {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    },
                ),
                Span::raw(format!("{} ", row.link_mark())),
                Span::raw(format!("{} ", row.label)),
                Span::styled(
                    row.presence.to_string(),
                    Style::default().fg(match row.presence {
                        membership::domain::Presence::Online => Color::Green,
                        membership::domain::Presence::Stale => Color::Yellow,
                        membership::domain::Presence::Offline => Color::DarkGray,
                    }),
                ),
            ]))
        })
        .collect();

    frame.render_widget(
        List::new(roster).block(Block::default().borders(Borders::ALL).title("peers")),
        split[1],
    );
}

fn draw_conversation(frame: &mut Frame<'_>, area: Rect, data: &ScreenData<'_>) {
    let mut lines: Vec<Line<'_>> = Vec::new();

    if data.conversation.is_empty() {
        lines.push(Line::from(Span::styled(
            "nothing here yet",
            Style::default().fg(Color::DarkGray),
        )));
    }

    for run in &data.conversation.authors {
        lines.push(Line::from(Span::styled(
            format!("── {} ──", run.label),
            Style::default()
                .fg(if run.is_local {
                    Color::Cyan
                } else {
                    Color::Magenta
                })
                .add_modifier(Modifier::BOLD),
        )));

        for entry in &run.entries {
            lines.push(match entry {
                Entry::Message { body, delivery, .. } => Line::from(vec![
                    Span::raw(format!("  {body}  ")),
                    Span::styled(
                        delivery_text(*delivery),
                        Style::default().fg(match delivery {
                            messaging::domain::DeliveryState::Failed(_) => Color::Red,
                            messaging::domain::DeliveryState::Pending => Color::Yellow,
                            _ => Color::DarkGray,
                        }),
                    ),
                ]),
                Entry::AbandonedRun { .. } => Line::from(Span::styled(
                    format!(
                        "  ⚠ {}",
                        entry
                            .abandoned_text(&run.label)
                            .unwrap_or_else(|| "messages were never received".to_owned())
                    ),
                    Style::default().fg(Color::Red),
                )),
            });
        }

        lines.push(Line::from(""));
    }

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title("messages — grouped by author; the domain provides no order across authors"),
        ),
        area,
    );
}

fn draw_notices(frame: &mut Frame<'_>, area: Rect, notices: &[Notice]) {
    let lines: Vec<Line<'_>> = notices
        .iter()
        .flat_map(|notice| {
            let style = match notice.level {
                NoticeLevel::Info => Style::default().fg(Color::Gray),
                NoticeLevel::Warning => Style::default().fg(Color::Yellow),
            };
            notice
                .text
                .lines()
                .map(|line| Line::from(Span::styled(line.to_owned(), style)))
                .collect::<Vec<_>>()
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title("notices")),
        area,
    );
}

fn draw_input(frame: &mut Frame<'_>, area: Rect, state: &UiState) {
    let (title, prompt) = match state.mode() {
        Mode::Browsing => ("press i to write, ? for help", String::new()),
        Mode::Composing => (
            "message — Enter sends, Esc cancels",
            state.input().to_owned(),
        ),
        Mode::RedeemingTicket => (
            "paste a join ticket — Enter joins, Esc cancels",
            state.input().to_owned(),
        ),
    };

    frame.render_widget(
        Paragraph::new(format!("> {prompt}"))
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn draw_overlay(frame: &mut Frame<'_>, state: &UiState, data: &ScreenData<'_>) {
    let (title, lines) = match state.overlay() {
        Overlay::None => return,
        Overlay::Help => ("help", help_lines()),
        Overlay::Fingerprints => (
            "fingerprints — compare these out of band",
            fingerprint_lines(data),
        ),
        Overlay::Ticket(ticket) => (
            "join ticket — hand this over out of band",
            ticket_lines(ticket),
        ),
        Overlay::Diagnostics => ("local diagnostics", diagnostic_lines(data)),
    };

    let area = centred(frame.area(), 80, 70);
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .block(Block::default().borders(Borders::ALL).title(title)),
        area,
    );
}

fn help_lines() -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = KeyBindings::HELP
        .iter()
        .map(|(keys, description)| {
            Line::from(vec![
                Span::styled(
                    format!("{keys:<20}"),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw((*description).to_owned()),
            ])
        })
        .collect();

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "what you should know",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    // S7 and S8: these are not decoration. A safeguard a user has to read the
    // source to learn is not a disclosure.
    for disclosure in Usage::DISCLOSURES {
        lines.push(Line::from(Span::styled(
            format!("• {disclosure}"),
            Style::default().fg(Color::Yellow),
        )));
    }

    lines
}

fn fingerprint_lines(data: &ScreenData<'_>) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            "you",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(data.local_fingerprint.to_owned()),
        Line::from(""),
    ];

    match data.selected_fingerprint {
        Some(fingerprint) => {
            lines.push(Line::from(Span::styled(
                "selected peer",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(fingerprint.to_owned()));
            lines.push(Line::from(""));
            lines.push(Line::from(
                "Read these to each other over a channel you already trust. If they match, press v to verify.",
            ));
        }
        None => lines.push(Line::from(
            "Select a direct conversation to see that peer's fingerprint.",
        )),
    }

    lines
}

fn ticket_lines(ticket: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(ticket.to_owned()),
        Line::from(""),
        Line::from("Anyone who pastes this can reach this peer. It expires in 24 hours."),
        Line::from("Handing it over announces this peer's addresses to whoever receives it."),
    ]
}

fn diagnostic_lines(data: &ScreenData<'_>) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            format!("profile  {}", data.profile),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
    ];

    lines.extend(
        data.diagnostics
            .iter()
            .map(|(label, value)| Line::from(format!("{label:<32}{value}"))),
    );

    lines
}

/// A box occupying `width`% by `height`% of `area`, centred.
fn centred(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height) / 2),
            Constraint::Percentage(height),
            Constraint::Percentage((100 - height) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width) / 2),
            Constraint::Percentage(width),
            Constraint::Percentage((100 - width) / 2),
        ])
        .split(vertical[1])[1]
}
