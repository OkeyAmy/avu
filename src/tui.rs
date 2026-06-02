use crate::{backend, cli::TuiArgs, domain::*, engine, hud, intent};
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
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
    fs,
    io::{self, Write},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

type RefreshResult = std::result::Result<CockpitState, String>;
type RefreshSender = mpsc::Sender<RefreshResult>;
type RefreshReceiver = mpsc::Receiver<RefreshResult>;

type PromptResult = std::result::Result<backend::BackendCommandResult, String>;
type PromptSender = mpsc::Sender<PromptResult>;
type PromptReceiver = mpsc::Receiver<PromptResult>;

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
    let mut state = initial_state_for_args(&args)?;
    let mut overlay = UiOverlay::default();
    let mut last_refresh = Instant::now();
    let mut session_log = create_session_log();
    let (refresh_tx, refresh_rx): (RefreshSender, RefreshReceiver) = mpsc::channel();
    let mut refresh_inflight = false;
    let (prompt_tx, prompt_rx): (PromptSender, PromptReceiver) = mpsc::channel();
    let mut prompt_inflight = false;

    push_operator_event(
        &mut state,
        EventKind::Idle,
        &format!("Avu session started — {:?} backend", args.backend),
        &mut session_log,
    );

    loop {
        while let Ok(result) = refresh_rx.try_recv() {
            refresh_inflight = false;
            match result {
                Ok(refreshed) => {
                    apply_refreshed_state(&mut state, refreshed);
                    overlay.message = "backend status refreshed".to_string();
                }
                Err(error) => {
                    overlay.message = format!("backend refresh failed: {}", compact(&error, 48));
                }
            }
        }

        while let Ok(result) = prompt_rx.try_recv() {
            prompt_inflight = false;
            match result {
                Ok(cmd_result) => {
                    let kind = if cmd_result.ok {
                        EventKind::ResponseStream
                    } else {
                        EventKind::Warning
                    };
                    let label = format!("backend: {}", compact(&cmd_result.summary, 120));
                    push_operator_event(&mut state, kind, &label, &mut session_log);
                    overlay.message = if cmd_result.ok {
                        format!(
                            "backend replied: {}",
                            compact(&cmd_result.summary, 48)
                        )
                    } else {
                        format!(
                            "backend failed: {}",
                            compact(&cmd_result.summary, 48)
                        )
                    };
                    if !refresh_inflight {
                        spawn_refresh(&args, &refresh_tx);
                        refresh_inflight = true;
                        last_refresh = Instant::now();
                    }
                }
                Err(error) => {
                    push_operator_event(
                        &mut state,
                        EventKind::Warning,
                        &error,
                        &mut session_log,
                    );
                    overlay.message = format!("backend error: {}", compact(&error, 48));
                }
            }
        }

        engine::apply_projection(&mut state);
        terminal.draw(|frame| render_with_overlay(frame, &state, Some(&overlay)))?;

        if last_refresh.elapsed() >= Duration::from_secs(5)
            && !overlay.is_typing()
            && !refresh_inflight
            && !prompt_inflight
        {
            spawn_refresh(&args, &refresh_tx);
            refresh_inflight = true;
            last_refresh = Instant::now();
        }

        if event::poll(Duration::from_millis(250))?
            && let Event::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            if overlay.input_mode.is_some() {
                match handle_input_key(
                    &mut state,
                    &args,
                    &mut overlay,
                    key.code,
                    &prompt_tx,
                    &mut prompt_inflight,
                    &mut session_log,
                )? {
                    CommandAction::Continue => {}
                    CommandAction::Quit => break,
                }
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char('b') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    run_voice_handoff(
                        &mut terminal,
                        &mut state,
                        &args,
                        &mut overlay,
                        &mut session_log,
                    )?;
                    if !refresh_inflight {
                        spawn_refresh(&args, &refresh_tx);
                        refresh_inflight = true;
                        last_refresh = Instant::now();
                    }
                }
                KeyCode::Char(':') => {
                    overlay.input_mode = Some(InputMode::Picker);
                    overlay.input.clear();
                    overlay.message =
                        "choose input: [c] CMD shell · [t] Chat · [/] slash".to_string();
                }
                KeyCode::Char('s') if !refresh_inflight && !prompt_inflight => {
                    spawn_refresh(&args, &refresh_tx);
                    refresh_inflight = true;
                    last_refresh = Instant::now();
                    overlay.message = "refreshing backend status...".to_string();
                }
                KeyCode::Char('i') => push_operator_event(
                    &mut state,
                    EventKind::Warning,
                    "interrupt requested; backend control routing not enabled yet",
                    &mut session_log,
                ),
                KeyCode::Char('m') => push_operator_event(
                    &mut state,
                    EventKind::Listening,
                    "voice status requested; use :/voice, :/tts, or :/stt to route through backend",
                    &mut session_log,
                ),
                _ => {}
            }
        }
    }

    if let Some(ref mut file) = session_log {
        let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{ts}] [session] Avu session ended");
    }

    Ok(())
}

fn initial_state_for_args(args: &TuiArgs) -> Result<CockpitState> {
    if let Some(fixture) = args.fixture.as_ref() {
        backend::load_fixture(fixture)
    } else {
        Ok(backend::quick_cockpit_state(args.backend))
    }
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
    prompt_tx: &PromptSender,
    prompt_inflight: &mut bool,
    session_log: &mut Option<fs::File>,
) -> Result<CommandAction> {
    match overlay.input_mode {
        Some(InputMode::Picker) => handle_picker_key(state, args, overlay, key_code),
        Some(InputMode::Command) | Some(InputMode::Chat) => {
            handle_text_input_key(state, args, overlay, key_code, prompt_tx, prompt_inflight, session_log)
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
            refresh_state(state, args)?;
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
    prompt_tx: &PromptSender,
    prompt_inflight: &mut bool,
    session_log: &mut Option<fs::File>,
) -> Result<CommandAction> {
    match key_code {
        KeyCode::Esc => {
            overlay.input_mode = None;
            overlay.input.clear();
            overlay.message = "input cancelled".to_string();
        }
        KeyCode::Enter => match overlay.input_mode {
            Some(InputMode::Command) => run_shell_input(state, overlay, session_log),
            Some(InputMode::Chat) => {
                route_chat_input(state, args, overlay, prompt_tx, prompt_inflight, session_log)
            }
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

fn run_shell_input(
    state: &mut CockpitState,
    overlay: &mut UiOverlay,
    session_log: &mut Option<fs::File>,
) {
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
    push_operator_event(state, kind, &label, session_log);
    overlay.message = if result.ok {
        format!("cmd ok: {}", compact(&result.summary, 48))
    } else {
        format!("cmd failed: {}", compact(&result.summary, 48))
    };
}

fn route_chat_input(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    prompt_tx: &PromptSender,
    prompt_inflight: &mut bool,
    session_log: &mut Option<fs::File>,
) {
    let prompt = overlay.input.trim().to_string();
    overlay.input_mode = None;
    overlay.input.clear();
    if prompt.is_empty() {
        overlay.message = "empty prompt, cancelled".to_string();
        return;
    }
    if *prompt_inflight {
        overlay.message =
            "already waiting for backend; wait for response or restart Avu".to_string();
        return;
    }
    push_operator_event(
        state,
        EventKind::Processing,
        &format!("sending: {}", compact(&prompt, 60)),
        session_log,
    );
    overlay.message = "sending to backend...".to_string();
    *prompt_inflight = true;
    spawn_prompt(args.clone(), prompt_tx.clone(), prompt);
}

fn spawn_prompt(args: TuiArgs, tx: PromptSender, prompt: String) {
    thread::spawn(move || {
        let result = backend::adapter_for(args.backend).route_prompt(&prompt);
        let _ = tx.send(Ok(result));
    });
}

fn run_voice_handoff(
    terminal: &mut TerminalSession,
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    session_log: &mut Option<fs::File>,
) -> Result<()> {
    push_operator_event(
        state,
        EventKind::Listening,
        "voice: handing terminal to Hermes; enable /voice on then press Ctrl+B there",
        session_log,
    );
    terminal.suspend_for(|| backend::run_voice_session(args.backend));
    refresh_state(state, args)?;
    overlay.message = "voice session returned to Avu".to_string();
    Ok(())
}

fn refresh_state(state: &mut CockpitState, args: &TuiArgs) -> Result<()> {
    let refreshed = state_for_args(args)?;
    apply_refreshed_state(state, refreshed);
    Ok(())
}

fn apply_refreshed_state(state: &mut CockpitState, mut refreshed: CockpitState) {
    let local_events: Vec<CockpitEvent> = state
        .events
        .iter()
        .filter(|event| is_operator_event(&event.label))
        .cloned()
        .collect();
    refreshed.events.extend(local_events);
    keep_recent_state_events(&mut refreshed, 12);
    *state = refreshed;
}

fn spawn_refresh(args: &TuiArgs, refresh_tx: &RefreshSender) {
    let args = args.clone();
    let refresh_tx = refresh_tx.clone();
    thread::spawn(move || {
        let result = state_for_args(&args).map_err(|error| error.to_string());
        let _ = refresh_tx.send(result);
    });
}

fn is_operator_event(label: &str) -> bool {
    label.starts_with("cmd: ")
        || label.starts_with("backend: ")
        || label.starts_with("voice: ")
        || label.starts_with("sending: ")
        || label.starts_with("Avu session ")
}

fn keep_recent_state_events(state: &mut CockpitState, limit: usize) {
    if state.events.len() > limit {
        let drop_count = state.events.len() - limit;
        state.events.drain(0..drop_count);
    }
}

fn push_operator_event(
    state: &mut CockpitState,
    kind: EventKind,
    label: &str,
    log: &mut Option<fs::File>,
) {
    let ts = chrono::Utc::now();
    state.events.push(CockpitEvent {
        at: ts,
        kind: kind.clone(),
        label: label.to_string(),
    });
    if let Some(file) = log.as_mut() {
        let label_ts = ts.format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{label_ts}] [{kind:?}] {label}");
    }
}

fn create_session_log() -> Option<fs::File> {
    let paths = crate::config::AvuPaths::discover();
    fs::create_dir_all(&paths.logs).ok()?;
    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
    let path = paths.logs.join(format!("avu-session-{timestamp}.log"));
    match fs::File::create(&path) {
        Ok(file) => {
            eprintln!("Avu session log: {}", path.display());
            Some(file)
        }
        Err(error) => {
            eprintln!("Avu session log disabled: {error}");
            None
        }
    }
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

    fn suspend_for<F>(&mut self, action: F)
    where
        F: FnOnce() -> backend::BackendCommandResult,
    {
        let _ = disable_raw_mode();
        let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen);
        let result = action();
        eprintln!("Avu voice handoff: {}", result.summary);
        let _ = enable_raw_mode();
        let _ = execute!(
            self.terminal.backend_mut(),
            EnterAlternateScreen,
            Clear(ClearType::All)
        );
        let _ = self.terminal.clear();
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
        Span::raw("[s]status [:]CMD/Chat [Ctrl+B]Hermes voice [m]help [q]quit"),
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
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        let action = handle_text_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Enter,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("command routes");

        assert!(action.is_continue());
        assert!(overlay.message.contains("sending to backend"));
        assert!(prompt_inflight);
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Processing
                && event.label.contains("sending: /voice")
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
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Char('c'),
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("cmd mode selected");
        assert_eq!(overlay.input_mode, Some(InputMode::Command));

        overlay.input_mode = Some(InputMode::Picker);
        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Char('t'),
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
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
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Backspace,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("backspace works");
        assert_eq!(overlay.input, "hell");
        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Delete,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("delete works");
        assert_eq!(overlay.input, "hel");
    }

    #[test]
    fn refresh_preserves_operator_events() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Fake,
            ..TuiArgs::default()
        };
        let mut state = CockpitState::fake_listening();
        state.events.clear();
        let mut session_log = None;
        push_operator_event(&mut state, EventKind::ResponseStream, "cmd: printf avu", &mut session_log);

        refresh_state(&mut state, &args).expect("refresh keeps local activity");

        assert!(
            state
                .events
                .iter()
                .any(|event| event.label == "cmd: printf avu")
        );
    }
}
