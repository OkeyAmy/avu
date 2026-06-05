use crate::{
    backend, backend_events::BackendTurnRequest, cli::TuiArgs, config, domain::*, engine, hud,
    intent, runtime, voice_activation,
};
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
    let mut session_log = create_session_log();
    let mut terminal = TerminalSession::start()?;
    let mut state = initial_state_for_args(&args)?;
    let mut overlay = UiOverlay::default();
    let mut last_refresh = Instant::now();
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
    write_state_snapshot(&mut session_log, "start", &state);

    loop {
        while let Ok(result) = refresh_rx.try_recv() {
            refresh_inflight = false;
            match result {
                Ok(refreshed) => {
                    write_state_snapshot(&mut session_log, "refresh", &refreshed);
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
                    apply_prompt_result(&mut state, &mut overlay, cmd_result, &mut session_log);
                    if !refresh_inflight {
                        spawn_refresh(&args, &refresh_tx);
                        refresh_inflight = true;
                        last_refresh = Instant::now();
                    }
                }
                Err(error) => {
                    push_operator_event(&mut state, EventKind::Warning, &error, &mut session_log);
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
                    overlay.clear_input();
                    overlay.message =
                        "choose input: [c] CMD shell · [t] Chat · [/] backend CLI".to_string();
                }
                KeyCode::Char('s') if !refresh_inflight && !prompt_inflight => {
                    spawn_refresh(&args, &refresh_tx);
                    refresh_inflight = true;
                    last_refresh = Instant::now();
                    overlay.message = "refreshing backend status...".to_string();
                }
                KeyCode::Char('w') => {
                    arm_keyboard_wake(&mut state, &mut overlay, &mut session_log);
                }
                KeyCode::Up if !overlay.is_typing() => {
                    overlay.activity_scroll = overlay.activity_scroll.saturating_add(1);
                }
                KeyCode::Down if !overlay.is_typing() => {
                    overlay.activity_scroll = overlay.activity_scroll.saturating_sub(1);
                }
                KeyCode::Char('a') if state.pending_approval.is_some() => {
                    arm_approval(&state, &args, &mut overlay);
                }
                KeyCode::Char('A') if state.pending_approval.is_some() => {
                    route_approval_response(
                        &mut state,
                        &args,
                        &mut overlay,
                        &prompt_tx,
                        &mut prompt_inflight,
                        &mut session_log,
                        "approve",
                    );
                }
                KeyCode::Char('r') if state.pending_approval.is_some() => {
                    route_approval_response(
                        &mut state,
                        &args,
                        &mut overlay,
                        &prompt_tx,
                        &mut prompt_inflight,
                        &mut session_log,
                        "reject",
                    );
                }
                KeyCode::Char('i') => {
                    push_operator_event(
                        &mut state,
                        EventKind::Warning,
                        "interrupt requested; backend control routing not enabled yet",
                        &mut session_log,
                    );
                }
                KeyCode::Char('m') => {
                    push_operator_event(
                        &mut state,
                        EventKind::Listening,
                        "voice status requested; use w to arm keyboard wake or Ctrl+B for Hermes mic voice mode",
                        &mut session_log,
                    );
                }
                _ => {}
            }
        }
    }

    if let Some(ref mut file) = session_log {
        let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{ts}] [session] Avu session ended");
        let _ = file.flush();
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
    input_cursor: usize,
    message: String,
    activity_scroll: usize,
    approval_armed_id: Option<String>,
}

impl UiOverlay {
    fn is_typing(&self) -> bool {
        self.input_mode.is_some()
    }

    fn clear_input(&mut self) {
        self.input.clear();
        self.input_cursor = 0;
    }

    fn insert_char(&mut self, ch: char) {
        let byte_index = self.cursor_byte_index();
        self.input.insert(byte_index, ch);
        self.input_cursor += 1;
    }

    fn backspace(&mut self) {
        if self.input_cursor == 0 {
            return;
        }
        let remove_at = self.nth_char_byte_index(self.input_cursor - 1);
        self.input.remove(remove_at);
        self.input_cursor -= 1;
    }

    fn delete(&mut self) {
        if self.input_cursor >= self.input_len() {
            return;
        }
        let remove_at = self.cursor_byte_index();
        self.input.remove(remove_at);
    }

    fn move_left(&mut self) {
        self.input_cursor = self.input_cursor.saturating_sub(1);
    }

    fn move_right(&mut self) {
        self.input_cursor = (self.input_cursor + 1).min(self.input_len());
    }

    fn move_home(&mut self) {
        self.input_cursor = 0;
    }

    fn move_end(&mut self) {
        self.input_cursor = self.input_len();
    }

    fn input_len(&self) -> usize {
        self.input.chars().count()
    }

    fn cursor_byte_index(&self) -> usize {
        self.nth_char_byte_index(self.input_cursor)
    }

    fn nth_char_byte_index(&self, char_index: usize) -> usize {
        self.input
            .char_indices()
            .nth(char_index)
            .map(|(index, _)| index)
            .unwrap_or(self.input.len())
    }

    fn visible_input_with_cursor(&self, max_chars: usize) -> String {
        let total = self.input_len();
        if total == 0 {
            return "▌".to_string();
        }
        let max_chars = max_chars.max(1);
        let half = max_chars / 2;
        let start = if total <= max_chars {
            0
        } else {
            self.input_cursor
                .saturating_sub(half)
                .min(total.saturating_sub(max_chars))
        };
        let end = (start + max_chars).min(total);
        let mut output = String::new();
        if start > 0 {
            output.push('…');
        }
        for (index, ch) in self.input.chars().enumerate().skip(start).take(end - start) {
            if index == self.input_cursor {
                output.push('▌');
            }
            output.push(ch);
        }
        if self.input_cursor >= end {
            output.push('▌');
        }
        if end < total {
            output.push('…');
        }
        output
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum InputMode {
    Picker,
    Command,
    Chat,
    Slash,
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
        Some(InputMode::Command) | Some(InputMode::Chat) | Some(InputMode::Slash) => {
            handle_text_input_key(
                state,
                args,
                overlay,
                key_code,
                prompt_tx,
                prompt_inflight,
                session_log,
            )
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
            overlay.clear_input();
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
            overlay.input_mode = Some(InputMode::Slash);
            overlay.clear_input();
            overlay.message = "Slash mode: type backend slash command, e.g. voice on".to_string();
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
            overlay.clear_input();
            overlay.message = "input cancelled".to_string();
        }
        KeyCode::Enter => match overlay.input_mode {
            Some(InputMode::Command) => run_shell_input(state, overlay, session_log),
            Some(InputMode::Chat) => route_chat_input(
                state,
                args,
                overlay,
                prompt_tx,
                prompt_inflight,
                session_log,
            )?,
            Some(InputMode::Slash) => route_slash_input(
                state,
                args,
                overlay,
                prompt_tx,
                prompt_inflight,
                session_log,
            )?,
            _ => {}
        },
        KeyCode::PageUp => {
            overlay.activity_scroll = overlay.activity_scroll.saturating_add(1);
        }
        KeyCode::PageDown => {
            overlay.activity_scroll = overlay.activity_scroll.saturating_sub(1);
        }
        KeyCode::Backspace | KeyCode::Delete => {
            if matches!(key_code, KeyCode::Backspace) {
                overlay.backspace();
            } else {
                overlay.delete();
            }
        }
        KeyCode::Left => overlay.move_left(),
        KeyCode::Right => overlay.move_right(),
        KeyCode::Home => overlay.move_home(),
        KeyCode::End => overlay.move_end(),
        KeyCode::Char(ch) => overlay.insert_char(ch),
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
    overlay.clear_input();
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

fn route_slash_input(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    prompt_tx: &PromptSender,
    prompt_inflight: &mut bool,
    session_log: &mut Option<fs::File>,
) -> Result<()> {
    let command = overlay.input.trim().trim_start_matches('/').trim();
    if command.is_empty() {
        overlay.clear_input();
        overlay.message = "empty slash command, cancelled".to_string();
    } else {
        overlay.input = format!("/{command}");
        overlay.input_cursor = overlay.input_len();
        route_chat_input(
            state,
            args,
            overlay,
            prompt_tx,
            prompt_inflight,
            session_log,
        )?;
    }
    Ok(())
}

fn arm_keyboard_wake(
    state: &mut CockpitState,
    overlay: &mut UiOverlay,
    session_log: &mut Option<fs::File>,
) {
    let backend_voice_config = match state.backend {
        BackendKind::Hermes => runtime::hermes_config_text(),
        _ => None,
    };
    arm_keyboard_wake_with_backend_voice_config(
        state,
        overlay,
        session_log,
        backend_voice_config.as_deref(),
    );
}

fn arm_keyboard_wake_with_backend_voice_config(
    state: &mut CockpitState,
    overlay: &mut UiOverlay,
    session_log: &mut Option<fs::File>,
    backend_voice_config: Option<&str>,
) {
    let paths = config::AvuPaths::discover();
    let avu_config = config::AvuConfig::load(&paths).unwrap_or_default();
    let status = voice_activation::activation_status(
        &avu_config,
        &state.backend_label,
        backend_voice_config,
    );
    let label = format!(
        "wake: armed keyboard activation for `{}`; say/type wake-prefixed commands or press Ctrl+B for backend voice",
        status.wake_phrase
    );
    push_operator_event(state, EventKind::Listening, &label, session_log);
    overlay.message = if status.backend_voice_available {
        format!("wake armed: {} · backend voice ready", status.wake_phrase)
    } else {
        format!(
            "wake armed: {} · verify backend voice with Ctrl+B",
            status.wake_phrase
        )
    };
}

fn arm_approval(state: &CockpitState, args: &TuiArgs, overlay: &mut UiOverlay) {
    let Some(approval) = state.pending_approval.as_ref() else {
        overlay.message = "no pending backend approval".to_string();
        return;
    };
    if !can_route_approval(state, args) {
        overlay.approval_armed_id = None;
        overlay.message = "approval is observe-only for this backend".to_string();
        return;
    }
    overlay.approval_armed_id = Some(approval.id.clone());
    overlay.message = "approval armed; press A to confirm or r to reject".to_string();
}

fn can_route_approval(state: &CockpitState, args: &TuiArgs) -> bool {
    if args.fixture.is_some() {
        return false;
    }
    if state.capabilities.permission_posture != PermissionPosture::RouteApprovalResponses {
        return false;
    }
    let Some(approval) = state.pending_approval.as_ref() else {
        return false;
    };
    if state.backend != approval.backend {
        return false;
    }
    match args.backend {
        crate::cli::BackendChoice::Hermes => approval.backend == BackendKind::Hermes,
        crate::cli::BackendChoice::Openclaw => approval.backend == BackendKind::OpenClaw,
        crate::cli::BackendChoice::Fake => approval.backend == BackendKind::Fake,
        crate::cli::BackendChoice::Auto => approval.backend == state.backend,
        crate::cli::BackendChoice::Remote => approval.backend == BackendKind::Remote,
    }
}

fn route_approval_response(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    prompt_tx: &PromptSender,
    prompt_inflight: &mut bool,
    session_log: &mut Option<fs::File>,
    decision: &str,
) {
    let Some(approval) = state.pending_approval.as_ref() else {
        overlay.message = "no pending backend approval".to_string();
        return;
    };
    if !can_route_approval(state, args) {
        overlay.message = "approval is observe-only for this backend".to_string();
        return;
    }
    if decision == "approve" && overlay.approval_armed_id.as_deref() != Some(approval.id.as_str()) {
        overlay.message = "approval requires arming first with a".to_string();
        return;
    }
    if *prompt_inflight {
        overlay.message = "backend still running; approval response preserved".to_string();
        return;
    }
    let prompt = format!("/{decision} {}", approval.id);
    push_operator_event(
        state,
        EventKind::ApprovalRouted,
        &format!("approval routed: {decision} {}", approval.id),
        session_log,
    );
    state.pending_approval = None;
    overlay.approval_armed_id = None;
    overlay.message = format!("approval {decision} routed to backend");
    *prompt_inflight = true;
    spawn_prompt(args.clone(), prompt_tx.clone(), prompt);
}

fn route_chat_input(
    state: &mut CockpitState,
    args: &TuiArgs,
    overlay: &mut UiOverlay,
    prompt_tx: &PromptSender,
    prompt_inflight: &mut bool,
    session_log: &mut Option<fs::File>,
) -> Result<()> {
    let prompt = overlay.input.trim().to_string();
    if prompt.is_empty() {
        overlay.clear_input();
        overlay.message = "empty prompt, cancelled".to_string();
        return Ok(());
    }
    let paths = config::AvuPaths::discover();
    let avu_config = config::AvuConfig::load(&paths).unwrap_or_default();
    let mut wake_context = WakeIntentContext {
        args,
        prompt_tx,
        prompt_inflight,
        session_log,
        wake_phrase: &avu_config.wake_phrase,
    };
    if handle_wake_intent(state, overlay, &prompt, &mut wake_context)? {
        return Ok(());
    }
    if *prompt_inflight {
        overlay.message = "backend still running; wait for response (draft preserved)".to_string();
        return Ok(());
    }
    overlay.clear_input();
    push_operator_event(
        state,
        EventKind::Processing,
        &format!("sending: {}", compact(&prompt, 60)),
        session_log,
    );
    overlay.message = "sending to backend...".to_string();
    *prompt_inflight = true;
    spawn_prompt(args.clone(), prompt_tx.clone(), prompt);
    Ok(())
}

struct WakeIntentContext<'a> {
    args: &'a TuiArgs,
    prompt_tx: &'a PromptSender,
    prompt_inflight: &'a mut bool,
    session_log: &'a mut Option<fs::File>,
    wake_phrase: &'a str,
}

fn handle_wake_intent(
    state: &mut CockpitState,
    overlay: &mut UiOverlay,
    prompt: &str,
    context: &mut WakeIntentContext<'_>,
) -> Result<bool> {
    if !is_wake_prefixed(prompt, context.wake_phrase) {
        return Ok(false);
    }

    let parsed = intent::parse_intent_with_wake_phrase(prompt, context.wake_phrase);
    overlay.clear_input();
    match parsed {
        intent::Intent::Status => {
            refresh_state(state, context.args)?;
            push_operator_event(
                state,
                EventKind::Listening,
                "wake intent: status refreshed",
                context.session_log,
            );
            overlay.message = "wake intent: status refreshed".to_string();
        }
        intent::Intent::Interrupt => {
            push_operator_event(
                state,
                EventKind::Warning,
                "wake intent: interrupt requested; backend control routing not enabled yet",
                context.session_log,
            );
            overlay.message =
                "wake intent: interrupt noted; backend routing not enabled".to_string();
        }
        intent::Intent::Approve => {
            arm_approval(state, context.args, overlay);
        }
        intent::Intent::ConfirmApprove => {
            if let Some(approval) = state.pending_approval.as_ref() {
                overlay.approval_armed_id = Some(approval.id.clone());
            }
            route_approval_response(
                state,
                context.args,
                overlay,
                context.prompt_tx,
                context.prompt_inflight,
                context.session_log,
                "approve",
            );
        }
        intent::Intent::Reject => {
            route_approval_response(
                state,
                context.args,
                overlay,
                context.prompt_tx,
                context.prompt_inflight,
                context.session_log,
                "reject",
            );
        }
        intent::Intent::ListSessions
        | intent::Intent::Pause
        | intent::Intent::Resume
        | intent::Intent::SwitchSession(_)
        | intent::Intent::Mute => {
            let label = format!("wake intent: {:?} is not routed by Avu yet", parsed);
            push_operator_event(state, EventKind::Warning, &label, context.session_log);
            overlay.message = compact(&label, 72);
        }
        intent::Intent::Unknown(command) => {
            let label = format!(
                "wake intent ignored: `{}` is not a safe Avu control command",
                compact(&command, 48)
            );
            push_operator_event(state, EventKind::Warning, &label, context.session_log);
            overlay.message = compact(&label, 72);
        }
    }
    Ok(true)
}

fn is_wake_prefixed(prompt: &str, wake_phrase: &str) -> bool {
    let lower = prompt.trim_start().to_lowercase();
    let phrase = wake_phrase.trim().to_lowercase();
    if phrase.is_empty() {
        return false;
    }
    let Some(rest) = lower.strip_prefix(&phrase) else {
        return false;
    };
    rest.is_empty()
        || rest
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace() || matches!(ch, ',' | '.' | ':' | ';' | '!' | '?'))
}

fn spawn_prompt(args: TuiArgs, tx: PromptSender, prompt: String) {
    thread::spawn(move || {
        let request = BackendTurnRequest::text(prompt);
        let result = backend::adapter_for(args.backend).route_turn(&request);
        let _ = tx.send(Ok(result));
    });
}

fn apply_prompt_result(
    state: &mut CockpitState,
    overlay: &mut UiOverlay,
    cmd_result: backend::BackendCommandResult,
    session_log: &mut Option<fs::File>,
) {
    let kind = if cmd_result.ok {
        EventKind::ResponseStream
    } else {
        EventKind::Warning
    };
    if cmd_result.events.is_empty() {
        let label = format!("backend: {}", cmd_result.summary);
        push_operator_event(state, kind, &label, session_log);
    } else {
        for event in &cmd_result.events {
            push_operator_event(
                state,
                event.kind.clone(),
                &format!("backend: {}", event.label),
                session_log,
            );
        }
    }
    if cmd_result.ok {
        push_operator_event(
            state,
            EventKind::Listening,
            "ready: listening for next turn",
            session_log,
        );
    }
    overlay.message = if cmd_result.ok {
        format!("backend replied: {}", compact(&cmd_result.summary, 48))
    } else {
        format!("backend failed: {}", compact(&cmd_result.summary, 48))
    };
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
    let backend_has_active_event = refreshed.events.iter().any(is_active_backend_event);
    let local_events: Vec<CockpitEvent> = state
        .events
        .iter()
        .filter(|event| is_operator_event(&event.label))
        .filter(|event| !(backend_has_active_event && event.label.starts_with("ready: ")))
        .cloned()
        .collect();
    refreshed.events.extend(local_events);
    keep_recent_state_events(&mut refreshed, 12);
    *state = refreshed;
}

fn is_active_backend_event(event: &CockpitEvent) -> bool {
    matches!(
        event.kind,
        EventKind::Processing
            | EventKind::ToolStart
            | EventKind::ApprovalRequested
            | EventKind::ApprovalRouted
            | EventKind::Speaking
    )
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
        || label.starts_with("backend-cli: ")
        || label.starts_with("voice: ")
        || label.starts_with("ready: ")
        || label.starts_with("sending: ")
        || label.starts_with("Avu session ")
}

fn keep_recent_state_events(state: &mut CockpitState, visible_limit: usize) {
    let retention_limit = visible_limit.max(200);
    if state.events.len() > retention_limit {
        let drop_count = state.events.len() - retention_limit;
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
        let _ = file.flush();
    }
}

fn write_state_snapshot(log: &mut Option<fs::File>, reason: &str, state: &CockpitState) {
    let Some(file) = log.as_mut() else {
        return;
    };
    let ts = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S");
    let _ = writeln!(
        file,
        "[{ts}] [snapshot:{reason}] backend={} model={} voice={} mode={:?} tools={} events={}",
        state.backend_label,
        state.model_label,
        state.voice_label,
        state.mode,
        state.tools_active,
        state.events.len()
    );
    for event in state.events.iter().rev().take(12).rev() {
        let event_ts = event.at.format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(
            file,
            "[{event_ts}] [runtime:{:?}] {}",
            event.kind, event.label
        );
    }
    let _ = file.flush();
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
        render_activity(frame, stack[1], state, overlay);
    } else {
        let main = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(66), Constraint::Length(42)])
            .split(vertical[0]);

        render_core(frame, main[0], state);
        render_activity(frame, main[1], state, overlay);
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
        Span::raw("[s]status [w]wake [:]CMD/Chat [Ctrl+B]Hermes voice [m]help [q]quit"),
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

fn render_activity(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    state: &CockpitState,
    overlay: Option<&UiOverlay>,
) {
    let mut events: Vec<&CockpitEvent> = state.events.iter().rev().take(12).collect();
    let scroll = overlay
        .map(|overlay| overlay.activity_scroll)
        .unwrap_or_default();
    if scroll > 0 && events.len() > scroll {
        events.drain(0..scroll);
    }

    let items: Vec<ListItem<'_>> = events
        .into_iter()
        .rev()
        .map(|event| {
            let color = match event.kind {
                EventKind::ToolStart | EventKind::ToolFinish => Color::Yellow,
                EventKind::ApprovalRequested => Color::LightRed,
                EventKind::Error => Color::Red,
                _ => Color::Cyan,
            };
            let wrapped_lines: Vec<String> = event
                .label
                .lines()
                .flat_map(|line| {
                    let max_width = usize::from(area.width.saturating_sub(12)).max(1);
                    if line.chars().count() <= max_width {
                        vec![line.to_string()]
                    } else {
                        line.chars()
                            .collect::<Vec<_>>()
                            .chunks(max_width)
                            .map(|chunk| chunk.iter().collect())
                            .collect()
                    }
                })
                .collect();

            let mut item_lines = Vec::new();
            if let Some(first_line) = wrapped_lines.first() {
                item_lines.push(Line::from(vec![
                    Span::styled(
                        event.at.format("[%H:%M:%S] ").to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(first_line.clone(), Style::default().fg(color)),
                ]));
                item_lines.extend(
                    wrapped_lines
                        .into_iter()
                        .skip(1)
                        .map(|line| Line::from(Span::styled(line, Style::default().fg(color)))),
                );
            } else {
                item_lines.push(Line::from(vec![
                    Span::styled(
                        event.at.format("[%H:%M:%S] ").to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(String::new(), Style::default().fg(color)),
                ]));
            }

            ListItem::new(item_lines)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" ACTIVITY LOG ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .highlight_symbol(">>")
        .highlight_spacing(ratatui::widgets::HighlightSpacing::Always);

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
                    InputMode::Command => format!(
                        "  │ CMD[{}/{}]: {}",
                        overlay.input_cursor,
                        overlay.input_len(),
                        overlay.visible_input_with_cursor(64)
                    ),
                    InputMode::Chat => format!(
                        "  │ Chat[{}/{}]: {}",
                        overlay.input_cursor,
                        overlay.input_len(),
                        overlay.visible_input_with_cursor(64)
                    ),
                    InputMode::Slash => format!(
                        "  │ Slash[{}/{}]: /{}",
                        overlay.input_cursor,
                        overlay.input_len(),
                        overlay.visible_input_with_cursor(64)
                    ),
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
    fn activity_log_wraps_long_backend_messages() {
        let mut state = CockpitState::fake_listening();
        state.events.clear();
        state.events.push(CockpitEvent {
            at: chrono::Utc::now(),
            kind: EventKind::ResponseStream,
            label: "backend: long reply with enough content to wrap across rows and still show tail-marker".to_string(),
        });

        let output = render_to_string(&state, 100, 32).expect("render should succeed");

        assert!(output.contains("tail-marker"));
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
            input_cursor: 6,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
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
        assert_eq!(overlay.input_mode, Some(InputMode::Chat));
        assert!(overlay.input.is_empty());
        assert!(overlay.message.contains("sending to backend"));
        assert!(prompt_inflight);
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Processing && event.label.contains("sending: /voice")
        }));
    }

    #[test]
    fn picker_exposes_cmd_and_chat_modes() {
        let args = TuiArgs::default();
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Picker),
            input: String::new(),
            input_cursor: 0,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
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

        overlay.input_mode = Some(InputMode::Picker);
        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Char('/'),
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("slash mode selected");
        assert_eq!(overlay.input_mode, Some(InputMode::Slash));
        assert!(overlay.input.is_empty());
        assert!(overlay.message.contains("Slash mode"));
    }

    #[test]
    fn slash_mode_routes_raw_slash_prompt_to_backend() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Fake,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Slash),
            input: "status --all".to_string(),
            input_cursor: 12,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Enter,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("slash command routes");

        assert_eq!(overlay.input_mode, Some(InputMode::Slash));
        assert!(overlay.input.is_empty());
        assert!(prompt_inflight);
        assert!(overlay.message.contains("sending to backend"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Processing && event.label.contains("sending: /status --all")
        }));
    }

    #[test]
    fn wake_status_intent_refreshes_locally_without_backend_prompt() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Fake,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        state.events.clear();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "Hey Avu, status".to_string(),
            input_cursor: 15,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let (prompt_tx, prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Enter,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("wake status routes locally");

        assert!(!prompt_inflight);
        assert!(prompt_rx.try_recv().is_err());
        assert!(overlay.input.is_empty());
        assert!(overlay.message.contains("status refreshed"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Listening && event.label == "wake intent: status refreshed"
        }));
    }

    #[test]
    fn unknown_wake_intent_is_not_sent_to_backend() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Fake,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        state.events.clear();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "Hey Avu, write arbitrary backend prompt".to_string(),
            input_cursor: 39,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let (prompt_tx, prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Enter,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("unknown wake intent is handled locally");

        assert!(!prompt_inflight);
        assert!(prompt_rx.try_recv().is_err());
        assert!(overlay.message.contains("wake intent ignored"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Warning && event.label.contains("wake intent ignored")
        }));
    }

    #[test]
    fn configured_wake_phrase_routes_through_intent_layer() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Fake,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        state.events.clear();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "Computer, status".to_string(),
            input_cursor: 16,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let (prompt_tx, prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;
        let prompt = overlay.input.clone();
        let mut context = WakeIntentContext {
            args: &args,
            prompt_tx: &prompt_tx,
            prompt_inflight: &mut prompt_inflight,
            session_log: &mut session_log,
            wake_phrase: "computer",
        };

        assert!(
            handle_wake_intent(&mut state, &mut overlay, &prompt, &mut context)
                .expect("configured wake phrase routes")
        );

        assert!(!prompt_inflight);
        assert!(prompt_rx.try_recv().is_err());
        assert!(overlay.message.contains("status refreshed"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Listening && event.label == "wake intent: status refreshed"
        }));
    }

    #[test]
    fn approval_keys_route_structured_pending_approval_to_backend() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Hermes,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        state.backend = BackendKind::Hermes;
        if let Some(approval) = state.pending_approval.as_mut() {
            approval.backend = BackendKind::Hermes;
        }
        let mut overlay = UiOverlay::default();
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        route_approval_response(
            &mut state,
            &args,
            &mut overlay,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
            "approve",
        );

        assert!(!prompt_inflight);
        assert!(state.pending_approval.is_some());
        assert!(overlay.message.contains("requires arming"));

        arm_approval(&state, &args, &mut overlay);
        assert_eq!(
            overlay.approval_armed_id.as_deref(),
            Some("approval-fixture-001")
        );

        route_approval_response(
            &mut state,
            &args,
            &mut overlay,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
            "approve",
        );

        assert!(prompt_inflight);
        assert!(state.pending_approval.is_none());
        assert!(overlay.message.contains("approval approve routed"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::ApprovalRouted && event.label.contains("approval-fixture-001")
        }));
    }

    #[test]
    fn approval_routing_rejects_mismatched_backend_source() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Hermes,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay::default();
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        arm_approval(&state, &args, &mut overlay);
        route_approval_response(
            &mut state,
            &args,
            &mut overlay,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
            "approve",
        );

        assert!(!prompt_inflight);
        assert!(state.pending_approval.is_some());
        assert!(overlay.message.contains("observe-only"));
    }

    #[test]
    fn approval_routing_rejects_fixture_spoofing_live_backend() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Hermes,
            fixture: Some(std::path::PathBuf::from(
                "fixtures/approval_destructive.json",
            )),
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        state.backend = BackendKind::Hermes;
        if let Some(approval) = state.pending_approval.as_mut() {
            approval.backend = BackendKind::Hermes;
        }
        let mut overlay = UiOverlay::default();
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        arm_approval(&state, &args, &mut overlay);
        route_approval_response(
            &mut state,
            &args,
            &mut overlay,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
            "approve",
        );

        assert!(!prompt_inflight);
        assert!(state.pending_approval.is_some());
        assert!(overlay.message.contains("observe-only"));
    }

    #[test]
    fn keyboard_wake_key_arms_listening_state() {
        let args = TuiArgs::default();
        let mut state = CockpitState::fake_listening();
        state.events.clear();
        let mut overlay = UiOverlay::default();
        let mut session_log = None;

        let _ = args;
        arm_keyboard_wake(&mut state, &mut overlay, &mut session_log);

        assert!(overlay.message.contains("wake armed"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Listening && event.label.contains("wake: armed keyboard")
        }));
    }

    #[test]
    fn keyboard_wake_reports_configured_hermes_voice_readiness() {
        let mut state = CockpitState::fake_listening();
        state.backend = BackendKind::Hermes;
        state.backend_label = "HERMES".to_string();
        state.events.clear();
        let mut overlay = UiOverlay::default();
        let mut session_log = None;
        let hermes_config = r#"
voice:
  record_key: ctrl+b
stt:
  enabled: true
  provider: groq
tts:
  provider: gemini
"#;

        arm_keyboard_wake_with_backend_voice_config(
            &mut state,
            &mut overlay,
            &mut session_log,
            Some(hermes_config),
        );

        assert!(overlay.message.contains("backend voice ready"));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Listening && event.label.contains("wake: armed keyboard")
        }));
    }

    #[test]
    fn footer_renders_visible_cursor_for_text_modes() {
        let state = CockpitState::fake_listening();
        let overlay = UiOverlay {
            input_mode: Some(InputMode::Slash),
            input: "status --all".to_string(),
            input_cursor: 6,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let backend = TestBackend::new(120, 36);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        terminal
            .draw(|frame| render_with_overlay(frame, &state, Some(&overlay)))
            .expect("draw overlay");
        let buffer = terminal.backend().buffer();
        let mut output = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                output.push_str(buffer[(x, y)].symbol());
            }
            output.push('\n');
        }

        assert!(output.contains("Slash[6/12]: /status▌ --all"));
    }

    #[test]
    fn input_mode_supports_backspace_and_delete() {
        let args = TuiArgs::default();
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "hello".to_string(),
            input_cursor: 5,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
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
            KeyCode::Left,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("left works");
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
    fn input_buffer_supports_left_right_home_end_midline_edit() {
        let args = TuiArgs::default();
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "alpha gamma".to_string(),
            input_cursor: 5,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        for key in [
            KeyCode::Char(' '),
            KeyCode::Char('b'),
            KeyCode::Char('e'),
            KeyCode::Char('t'),
            KeyCode::Char('a'),
            KeyCode::End,
            KeyCode::Left,
            KeyCode::Backspace,
            KeyCode::Right,
            KeyCode::Char('!'),
            KeyCode::Home,
            KeyCode::Delete,
        ] {
            handle_input_key(
                &mut state,
                &args,
                &mut overlay,
                key,
                &prompt_tx,
                &mut prompt_inflight,
                &mut session_log,
            )
            .expect("edit key works");
        }

        assert_eq!(overlay.input, "lpha beta gama!");
        assert_eq!(overlay.input_cursor, 0);
    }

    #[test]
    fn avu_session_log_is_created_and_grows_with_runtime_snapshot() {
        let temp = tempfile::NamedTempFile::new().expect("temp log");
        let mut log = Some(temp.reopen().expect("reopen log"));
        let mut state = CockpitState::fake_listening();
        state.events.clear();
        state.events.push(CockpitEvent {
            at: chrono::Utc::now(),
            kind: EventKind::ToolStart,
            label: "backend runtime tool event".to_string(),
        });

        write_state_snapshot(&mut log, "test", &state);
        drop(log);
        let text = std::fs::read_to_string(temp.path()).expect("read log");

        assert!(text.contains("[snapshot:test]"));
        assert!(text.contains("backend="));
        assert!(text.contains("backend runtime tool event"));
    }

    #[test]
    fn chat_preserves_next_message_while_backend_is_busy() {
        let args = TuiArgs {
            backend: crate::cli::BackendChoice::Fake,
            ..Default::default()
        };
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: "next message".to_string(),
            input_cursor: 12,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = true;
        let mut session_log = None;

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::Enter,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("busy backend keeps draft");

        assert_eq!(overlay.input, "next message");
        assert!(overlay.message.contains("backend still running"));
        assert!(overlay.message.contains("draft preserved"));
    }

    #[test]
    fn successful_backend_reply_returns_cockpit_to_listening() {
        let mut state = CockpitState::fake_listening();
        state.pending_approval = None;
        state.events.clear();
        state.events.push(CockpitEvent::now(
            EventKind::Processing,
            "sending: hello backend",
        ));
        let mut overlay = UiOverlay::default();
        let mut session_log = None;

        apply_prompt_result(
            &mut state,
            &mut overlay,
            backend::BackendCommandResult {
                ok: true,
                summary: "done".to_string(),
                audio_path: None,
                events: Vec::new(),
            },
            &mut session_log,
        );
        engine::apply_projection(&mut state);

        assert_eq!(state.mode, CockpitMode::Listening);
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::Listening && event.label == "ready: listening for next turn"
        }));
    }

    #[test]
    fn backend_reply_preserves_full_progress_events() {
        let mut state = CockpitState::fake_listening();
        state.pending_approval = None;
        state.events.clear();
        let mut overlay = UiOverlay::default();
        let mut session_log = None;

        apply_prompt_result(
            &mut state,
            &mut overlay,
            backend::BackendCommandResult {
                ok: true,
                summary: "session avu-tui running".to_string(),
                audio_path: None,
                events: vec![
                    backend::BackendCommandEvent {
                        kind: EventKind::Processing,
                        label: "session avu-tui running".to_string(),
                    },
                    backend::BackendCommandEvent {
                        kind: EventKind::ToolStart,
                        label: "tool dispatch start: search.files with full generated command text"
                            .to_string(),
                    },
                    backend::BackendCommandEvent {
                        kind: EventKind::ResponseStream,
                        label: "final response ready: full final answer text".to_string(),
                    },
                ],
            },
            &mut session_log,
        );

        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::ToolStart
                && event
                    .label
                    .contains("tool dispatch start: search.files with full generated command text")
        }));
        assert!(state.events.iter().any(|event| {
            event.kind == EventKind::ResponseStream
                && event
                    .label
                    .contains("final response ready: full final answer text")
        }));
        assert!(overlay.message.contains("backend replied"));
    }

    #[test]
    fn state_event_retention_keeps_full_activity_history_beyond_visible_rows() {
        let mut state = CockpitState::fake_listening();
        state.events = (0..40)
            .map(|index| CockpitEvent::now(EventKind::Processing, format!("event-{index}")))
            .collect();

        keep_recent_state_events(&mut state, 12);

        assert_eq!(state.events.len(), 40);
        assert_eq!(
            state.events.first().map(|event| event.label.as_str()),
            Some("event-0")
        );
    }

    #[test]
    fn refresh_drops_stale_ready_when_backend_reports_active_state() {
        let mut state = CockpitState::fake_listening();
        state.pending_approval = None;
        state.events.clear();
        state.events.push(CockpitEvent::now(
            EventKind::Listening,
            "ready: listening for next turn",
        ));

        let mut refreshed = CockpitState::fake_listening();
        refreshed.pending_approval = None;
        refreshed.events.clear();
        refreshed.events.push(CockpitEvent::now(
            EventKind::ApprovalRequested,
            "approval requested for shell command",
        ));

        apply_refreshed_state(&mut state, refreshed);
        engine::apply_projection(&mut state);

        assert_eq!(state.mode, CockpitMode::ApprovalNeeded);
        assert!(
            !state
                .events
                .iter()
                .any(|event| event.label == "ready: listening for next turn")
        );
    }

    #[test]
    fn refresh_drops_stale_ready_when_backend_reports_speaking() {
        let mut state = CockpitState::fake_listening();
        state.pending_approval = None;
        state.events.clear();
        state.events.push(CockpitEvent::now(
            EventKind::Listening,
            "ready: listening for next turn",
        ));

        let mut refreshed = CockpitState::fake_listening();
        refreshed.pending_approval = None;
        refreshed.events.clear();
        refreshed.events.push(CockpitEvent::now(
            EventKind::Speaking,
            "tools.voice_mode: TTS audio playback started",
        ));

        apply_refreshed_state(&mut state, refreshed);
        engine::apply_projection(&mut state);

        assert_eq!(state.mode, CockpitMode::Speaking);
        assert!(
            !state
                .events
                .iter()
                .any(|event| event.label == "ready: listening for next turn")
        );
    }

    #[test]
    fn input_mode_can_scroll_activity_log() {
        let args = TuiArgs::default();
        let mut state = CockpitState::fake_listening();
        let mut overlay = UiOverlay {
            input_mode: Some(InputMode::Chat),
            input: String::new(),
            input_cursor: 0,
            message: String::new(),
            activity_scroll: 0,
            approval_armed_id: None,
        };
        let (prompt_tx, _prompt_rx) = mpsc::channel();
        let mut prompt_inflight = false;
        let mut session_log = None;

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::PageUp,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("page up scrolls");
        assert_eq!(overlay.activity_scroll, 1);

        handle_input_key(
            &mut state,
            &args,
            &mut overlay,
            KeyCode::PageDown,
            &prompt_tx,
            &mut prompt_inflight,
            &mut session_log,
        )
        .expect("page down scrolls");
        assert_eq!(overlay.activity_scroll, 0);
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
        push_operator_event(
            &mut state,
            EventKind::ResponseStream,
            "cmd: printf avu",
            &mut session_log,
        );

        refresh_state(&mut state, &args).expect("refresh keeps local activity");

        assert!(
            state
                .events
                .iter()
                .any(|event| event.label == "cmd: printf avu")
        );
    }
}
