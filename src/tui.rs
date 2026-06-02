use crate::{backend, cli::TuiArgs, domain::*, engine, hud, intent};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::{CrosstermBackend, TestBackend},
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};
use std::{
    io,
    time::{Duration, Instant},
};

pub fn run(args: TuiArgs) -> Result<()> {
    if args.once {
        let state = state_for_args(&args)?;
        let output = render_to_string(&state, 120, 36)?;
        println!("{output}");
        return Ok(());
    }

    run_interactive(args)
}

fn state_for_args(args: &TuiArgs) -> Result<CockpitState> {
    if let Some(fixture) = args.fixture.as_ref() {
        backend::load_fixture(fixture)
    } else {
        Ok(backend::adapter_for(args.backend).cockpit_state())
    }
}

fn run_interactive(args: TuiArgs) -> Result<()> {
    let mut terminal = TerminalSession::start()?;
    let mut state = state_for_args(&args)?;
    let mut overlay = UiOverlay::default();
    let mut last_refresh = Instant::now();

    loop {
        engine::apply_projection(&mut state);
        terminal.draw(|frame| render_with_overlay(frame, &state, Some(&overlay)))?;

        if last_refresh.elapsed() >= Duration::from_secs(2) && !overlay.is_typing() {
            state = state_for_args(&args)?;
            last_refresh = Instant::now();
        }

        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if overlay.input_mode.is_some() {
                match handle_input_key(&mut state, &args, &mut overlay, key.code)? {
                    CommandAction::Continue => {}
                    CommandAction::Quit => break,
                }
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char(':') => {
                    overlay.input_mode = Some(InputMode::Picker);
                    overlay.input.clear();
                    overlay.message =
                        "choose input: [c] CMD shell · [t] Chat · [/] slash".to_string();
                }
                KeyCode::Char('s') => {
                    state = state_for_args(&args)?;
                    overlay.message = "status refreshed from backend".to_string();
                }
                KeyCode::Char('i') => push_operator_event(
                    &mut state,
                    EventKind::Warning,
                    "interrupt requested; backend control routing not enabled yet",
                ),
                KeyCode::Char('m') => push_operator_event(
                    &mut state,
                    EventKind::Listening,
                    "voice status requested; use :/voice, :/tts, or :/stt to route through backend",
                ),
                _ => {}
            }
        }
    }

    Ok(())
}

#[derive(Debug, Default)]
struct UiOverlay {
    input_mode: Option<InputMode>,
    input: String,
    message: String,
}

impl UiOverlay {
    fn is_typing(&self) -> bool {
        self.input_mode.is_some()
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InputMode {
    Picker,
    Command,
    Chat,
}

enum CommandAction {
    Continue,
    Quit,
}

impl CommandAction {
    #[cfg(test)]
    fn is_continue(&self) -> bool {
        matches!(self, Self::Continue)
    }
}

fn handle_input_key(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    key_code: KeyCode,
) -> Result<CommandAction> {
    match overlay.input_mode {
        Some(InputMode::Picker) => handle_picker_key(state, args, overlay, key_code),
        Some(InputMode::Command) | Some(InputMode::Chat) => {
            handle_text_input_key(state, args, overlay, key_code)
        }
        None => Ok(CommandAction::Continue),
    }
}

fn handle_picker_key(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    key_code: KeyCode,
) -> Result<CommandAction> {
    match key_code {
        KeyCode::Esc => {
            overlay.input_mode = None;
            overlay.input.clear();
            overlay.message = "input cancelled".to_string();
        }
        KeyCode::Char('c') => {
            overlay.input_mode = Some(InputMode::Command);
            overlay.message = "CMD mode: type a shell command, Enter to run".to_string();
        }
        KeyCode::Char('t') => {
            overlay.input_mode = Some(InputMode::Chat);
            overlay.message = "Chat mode: type a message for Hermes/OpenClaw".to_string();
        }
        KeyCode::Char('/') => {
            overlay.input_mode = Some(InputMode::Chat);
            overlay.input.push('/');
            overlay.message = "Chat slash mode: send a backend slash command".to_string();
        }
        KeyCode::Char('s') => {
            *state = state_for_args(args)?;
            overlay.input_mode = None;
            overlay.message = "status refreshed from backend".to_string();
        }
        KeyCode::Char('q') => return Ok(CommandAction::Quit),
        _ => {}
    }
    Ok(CommandAction::Continue)
}

fn handle_text_input_key(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    key_code: KeyCode,
) -> Result<CommandAction> {
    match key_code {
        KeyCode::Esc => {
            overlay.input_mode = None;
            overlay.input.clear();
            overlay.message = "input cancelled".to_string();
        }
        KeyCode::Enter => match overlay.input_mode {
            Some(InputMode::Command) => run_shell_input(state, overlay),
            Some(InputMode::Chat) => route_chat_input(state, args, overlay),
            _ => {}
        },
        KeyCode::Backspace | KeyCode::Delete => {
            overlay.input.pop();
        }
        KeyCode::Char(ch) => overlay.input.push(ch),
        _ => {}
    }
    Ok(CommandAction::Continue)
}

fn run_shell_input(state: &mut CockpitState, overlay: &mut UiOverlay) {
    let command = overlay.input.trim().to_string();
    overlay.input_mode = None;
    overlay.input.clear();
    let result = backend::run_shell_command(&command);
    let kind = if result.ok {
        EventKind::ResponseStream
    } else {
        EventKind::Warning
    };
    let label = format!("cmd: {}", compact(&result.summary, 120));
    push_operator_event(state, kind, &label);
    overlay.message = if result.ok {
        format!("cmd ok: {}", compact(&result.summary, 48))
    } else {
        format!("cmd failed: {}", compact(&result.summary, 48))
    };
}

fn route_chat_input(state: &mut CockpitState, args: &TuiArgs, overlay: &mut UiOverlay) {
    let prompt = overlay.input.trim().to_string();
    overlay.input_mode = None;
    overlay.input.clear();
    route_backend_prompt(state, args, overlay, &prompt);
}

fn route_backend_prompt(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    prompt: &str,
) {
    overlay.message = format!("routing to backend: {}", compact(prompt, 32));
    let result = backend::adapter_for(args.backend).route_prompt(prompt);
    let kind = if result.ok {
        EventKind::ResponseStream
    } else {
        EventKind::Warning
    };
    let label = format!("backend: {}", compact(&result.summary, 120));
    push_operator_event(state, kind, &label);
    overlay.message = if result.ok {
        format!("backend replied: {}", compact(&result.summary, 48))
    } else {
        format!("backend failed: {}", compact(&result.summary, 48))
    };
}

fn push_operator_event(state: &mut CockpitState, kind: EventKind, label: &str) {
    state.events.push(CockpitEvent {
        at: chrono::Utc::now(),
        kind,
        label: label.to_string(),
    });
}

struct TerminalSession {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

impl TerminalSession {
    fn start() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        Ok(Self { terminal })
    }

    fn draw<F>(&mut self, f: F) -> io::Result<()>
    where
        F: FnOnce(&mut ratatui::Frame<'_>),
    {
        self.terminal.draw(f).map(|_| ())
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let _ = self.terminal.show_cursor();
    }
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
    render_with_overlay(frame, state, None);
}

fn render_with_overlay(
    frame: &mut ratatui::Frame<'_>,
    state: &CockpitState,
    overlay: Option<&UiOverlay>,
) {
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
    render_footer(frame, vertical[1], state, overlay);
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
        Span::raw("[s]status [:]CMD/Chat [m]voice help [q]quit"),
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

fn render_footer(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &CockpitState,
    overlay: Option<&UiOverlay>,
) {
    let command_text = overlay
        .map(|overlay| {
            if let Some(input_mode) = overlay.input_mode {
                match input_mode {
                    InputMode::Picker => {
                        "  │ INPUT: [c] CMD  [t] Chat  [/] Slash  [Esc] cancel".to_string()
                    }
                    InputMode::Command => format!("  │ CMD: {}", overlay.input),
                    InputMode::Chat => format!("  │ Chat: {}", overlay.input),
                }
            } else if overlay.message.is_empty() {
                "  │ PRESS : for CMD/Chat".to_string()
            } else {
                format!("  │ {}", overlay.message)
            }
        })
        .unwrap_or_default();
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
        Span::styled(command_text, Style::default().fg(Color::LightGreen)),
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

    #[test]
    fn command_mode_routes_slash_prompts_to_backend() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Fake,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "/voice".to_string(),
            message: String::new(),
        };

        let action = handle_text_input_key(&mut state, &args, &mut overlay, KeyCode::Enter)
            .expect("command routes");

        assert!(action.is_continue());
        assert!(overlay.message.contains("backend replied"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::ResponseStream
                && event.label.contains("fixture backend received: /voice")
        }));
    }

    #[test]
    fn picker_exposes_cmd_and_chat_modes() {
        let args = TuiArgs::default();
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Picker),
            input: String::new(),
            message: String::new(),
        };

        handle_input_key(&mut state, &args, &mut overlay, KeyCode::Char('c'))
            .expect("cmd mode selected");
        assert_eq!(overlay.input_mode, Some(InputMode::Command));

        overlay.input_mode = Some(InputMode::Picker);
        handle_input_key(&mut state, &args, &mut overlay, KeyCode::Char('t'))
            .expect("chat mode selected");
        assert_eq!(overlay.input_mode, Some(InputMode::Chat));
    }

    #[test]
    fn input_mode_supports_backspace_and_delete() {
        let args = TuiArgs::default();
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "hello".to_string(),
            message: String::new(),
        };

        handle_input_key(&mut state, &args, &mut overlay, KeyCode::Backspace)
            .expect("backspace works");
        assert_eq!(overlay.input, "hell");
        handle_input_key(&mut state, &args, &mut overlay, KeyCode::Delete).expect("delete works");
        assert_eq!(overlay.input, "hel");
    }
}
