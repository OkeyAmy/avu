use crate::{backend, cli::StatusArgs, engine};
use anyhow::Result;

pub fn print_status(args: StatusArgs) -> Result<()> {
    let mut state = if let Some(fixture) = args.fixture.as_ref() {
        backend::load_fixture(fixture)?
    } else {
        backend::adapter_for(args.backend).cockpit_state()
    };
    engine::apply_projection(&mut state);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&state)?);
    } else {
        println!("Avu status");
        println!("Backend: {}", state.backend_label);
        println!("Model: {}", state.model_label);
        println!("Voice: {}", state.voice_label);
        println!("Tools active: {}", state.tools_active);
        println!("Capabilities:");
        println!("  events: {}", state.capabilities.events);
        println!("  approvals: {}", state.capabilities.approvals);
        println!("  interrupt: {}", state.capabilities.interrupt);
        println!("  pause_resume: {}", state.capabilities.pause_resume);
        println!("  sessions_list: {}", state.capabilities.sessions_list);
        if !state.capabilities.notes.is_empty() {
            println!("Notes:");
            for note in &state.capabilities.notes {
                println!("  - {note}");
            }
        }
        if let Some(approval) = state.pending_approval {
            println!("Pending approval: {}", approval.summary);
            if let Some(command) = approval.command {
                println!("Command: {command}");
            }
        }
    }

    Ok(())
}
