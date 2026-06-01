use crate::{backend, cli::TuiArgs, domain::*, engine, hud, intent};
use anyhow::Result;
use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

pub fn run(args: TuiArgs) -> Result<()> {
    if !args.once {
        eprintln!("interactive TUI loop is not enabled yet; rendering one architecture frame");
    }

    let state = if let Some(fixture) = args.fixture.as_ref() {
        backend::load_fixture(fixture)?
    } else {
        backend::adapter_for(args.backend).cockpit_state()
    };

    // This first architecture milestone renders one deterministic frame. The renderer is
    // intentionally backend-agnostic so live Hermes/OpenClaw adapters and fixtures share UI.
    let output = render_to_string(&state, 120, 36)?;
    println!("{output}");
    Ok(())
}

pub fn render_to_string(state: &CockpitState, width: u16, height: u16) -> Result<String> {
    let mut state = state.clone();
    engine::apply_projection(&mut state);
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend)?;
    terminal.draw(|frame| render(frame, &state))?;
    let buffer = terminal.backend().buffer();
    let mut lines = Vec::new();
    for y in 0..buffer.area.height {
        let mut line = String::new();
        for x in 0..buffer.area.width {
            line.push_str(buffer[(x, y)].symbol());
        }
        lines.push(line.trim_end().to_string());
    }
    Ok(lines.join("\n"))
}

fn render(frame: &mut ratatui::Frame<'_>, state: &CockpitState) {
    let root = frame.area();
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(root);

    if root.width < 100 {
        let stack = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(14), Constraint::Length(12)])
            .split(vertical[0]);
        render_core(frame, stack[0], state);
        render_activity(frame, stack[1], state);
    } else {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(66), Constraint::Length(42)])
            .split(vertical[0]);

        render_core(frame, main[0], state);
        render_activity(frame, main[1], state);
    }
    render_footer(frame, vertical[1], state);
}

fn render_core(frame: &mut ratatui::Frame<'_>, area: Rect, state: &CockpitState) {
    let core_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(if state.pending_approval.is_some() {
                8
            } else {
                4
            }),
        ])
        .split(area);

    let metrics = Paragraph::new(Line::from(vec![
        Span::styled("BACKEND ", Style::default().fg(Color::Cyan)),
        Span::raw(state.backend_label.clone()),
        Span::styled("  │ MODEL ", Style::default().fg(Color::Cyan)),
        Span::raw(state.model_label.clone()),
        Span::styled("  │ EVENTS ", Style::default().fg(Color::Cyan)),
        Span::raw(state.events.len().to_string()),
    ]))
    .alignment(Alignment::Center)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(metrics, core_chunks[0]);

    let activity = backend_activity(state);
    let hud = hud::render_hud(
        &state.mode,
        core_chunks[1].width.saturating_sub(2) as usize,
        core_chunks[1].height.saturating_sub(2) as usize,
        activity.min(100),
    );
    let hud_lines: Vec<Line<'_>> = hud
        .lines
        .into_iter()
        .map(|line| {
            Line::from(Span::styled(
                line,
                Style::default().fg(mode_color(&state.mode)),
            ))
        })
        .collect();

    let radar = Paragraph::new(hud_lines)
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .title(" AVU EVENT RADAR ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
    frame.render_widget(radar, core_chunks[1]);

    render_control_sheet(frame, core_chunks[2], state);
}

fn render_control_sheet(frame: &mut ratatui::Frame<'_>, area: Rect, state: &CockpitState) {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            "KEYS ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("[s]status [i]interrupt [m]mute [q]quit"),
    ])];

    if let Some(approval) = state.pending_approval.as_ref() {
        let command = approval.command.as_deref().unwrap_or("backend action");
        let approve_intent = intent::parse_intent("approve");
        let reject_intent = intent::parse_intent("reject");
        let gate = intent::approval_gate(&approve_intent, Some(approval), false);
        lines.push(Line::from(vec![
            Span::styled(
                "APPROVAL ",
                Style::default()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(compact(&approval.summary, 54)),
        ]));
        lines.push(Line::from(vec![
            Span::styled("ACTION   ", Style::default().fg(Color::Yellow)),
            Span::raw(command.to_string()),
        ]));
        lines.push(Line::from(match gate {
            intent::ApprovalGate::RequireSecondConfirmation { .. } => vec![
                Span::styled("SAFETY   ", Style::default().fg(Color::LightRed)),
                Span::raw(format!(
                    "[a] arm {:?}  [A] confirm  [r] {:?}",
                    approve_intent, reject_intent
                )),
            ],
            _ => vec![
                Span::styled("ROUTE    ", Style::default().fg(Color::Cyan)),
                Span::raw(format!(
                    "[a] {:?} via backend  [r] {:?} via backend",
                    approve_intent, reject_intent
                )),
            ],
        }));
    } else {
        lines.push(Line::from(vec![
            Span::styled("CONTROL ", Style::default().fg(Color::Cyan)),
            Span::raw("no pending backend approval; approval keys disabled"),
        ]));
    }

    let sheet = Paragraph::new(lines).block(
        Block::default()
            .title(" OPERATOR CONTROL ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if state.pending_approval.is_some() {
                Color::LightRed
            } else {
                Color::Cyan
            })),
    );
    frame.render_widget(sheet, area);
}

fn compact(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_string();
    }
    let mut output: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    output.push('…');
    output
}

fn render_activity(frame: &mut ratatui::Frame<'_>, area: Rect, state: &CockpitState) {
    let items: Vec<ListItem<'_>> = state
        .events
        .iter()
        .rev()
        .take(12)
        .map(|event| {
            let color = match event.kind {
                EventKind::ToolStart | EventKind::ToolFinish => Color::Yellow,
                EventKind::ApprovalRequested => Color::LightRed,
                EventKind::Error => Color::Red,
                _ => Color::Cyan,
            };
            ListItem::new(Line::from(vec![
                Span::styled(
                    event.at.format("[%H:%M:%S] ").to_string(),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(event.label.clone(), Style::default().fg(color)),
            ]))
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .title(" ACTIVITY LOG ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(list, area);
}

fn render_footer(frame: &mut ratatui::Frame<'_>, area: Rect, state: &CockpitState) {
    let footer = Paragraph::new(Line::from(vec![
        Span::styled(" BACKEND: ", Style::default().fg(Color::Cyan)),
        Span::raw(state.backend_label.clone()),
        Span::styled("  │ INPUT: ", Style::default().fg(Color::Cyan)),
        Span::raw(state.voice_label.clone()),
        Span::styled("  │ MODEL: ", Style::default().fg(Color::Cyan)),
        Span::raw(state.model_label.clone()),
        Span::styled("  │ TOOLS: ", Style::default().fg(Color::Cyan)),
        Span::styled(
            format!("{} active", state.tools_active),
            Style::default().fg(Color::Yellow),
        ),
    ]))
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
    );
    frame.render_widget(footer, area);
}

fn backend_activity(state: &CockpitState) -> u8 {
    if state.pending_approval.is_some() {
        return 95;
    }
    match state.mode {
        CockpitMode::Disconnected => 0,
        CockpitMode::Error => 15,
        CockpitMode::Idle => 25,
        CockpitMode::ToolActive | CockpitMode::Processing => 80,
        CockpitMode::ApprovalNeeded => 95,
        CockpitMode::Listening | CockpitMode::WakeDetected | CockpitMode::Speaking => 70,
    }
}

fn mode_color(mode: &CockpitMode) -> Color {
    match mode {
        CockpitMode::ApprovalNeeded | CockpitMode::Error => Color::LightRed,
        CockpitMode::ToolActive | CockpitMode::Processing | CockpitMode::WakeDetected => {
            Color::Yellow
        }
        CockpitMode::Disconnected => Color::DarkGray,
        _ => Color::Cyan,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_approval_cockpit_state() {
        let state = CockpitState::fake_listening();
        let output = render_to_string(&state, 100, 32).expect("render should succeed");
        assert!(output.contains("AVU EVENT RADAR"));
        assert!(output.contains("APPROVAL"));
        assert!(output.contains("ACTIVITY LOG"));
        assert!(output.contains("BACKEND:"));
    }
}
