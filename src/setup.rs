use crate::{backend::adapter_for, cli::SetupArgs, config::AvuPaths};
use anyhow::Result;

pub fn run(args: SetupArgs) -> Result<()> {
    let paths = AvuPaths::discover();
    let adapter = adapter_for(args.backend);
    let capability = adapter.probe();

    println!("Avu setup");
    println!("=========");
    println!("Mode: {}", if args.quick { "quick" } else { "full" });
    println!("Config home: {}", paths.home.display());
    println!();

    print_step(
        1,
        "Install and PATH check",
        true,
        "avu binary is running from this shell",
    );
    print_step(
        2,
        "Backend selection",
        true,
        &format!("selected backend adapter: {}", adapter.label()),
    );
    print_step(
        3,
        "Backend health verification",
        capability.reachable,
        if capability.reachable {
            "backend reachable or fixture backend active"
        } else {
            "backend missing/unhealthy; use hermes setup or openclaw onboard, then rerun avu setup --quick"
        },
    );
    print_step(
        4,
        "Capability discovery",
        capability.reachable,
        if capability.reachable {
            "events/approvals/control capability snapshot produced"
        } else {
            "backend unreachable; capability snapshot is degraded and control actions stay disabled"
        },
    );
    print_step(
        5,
        "Permission posture",
        true,
        "defaulting to backend-owned approvals; Avu only routes explicit responses",
    );
    print_step(
        6,
        "Wake and microphone",
        true,
        "keyboard-only fallback enabled; wake can be enabled later",
    );
    print_step(
        7,
        "Safety rehearsal",
        capability.approvals,
        if capability.approvals {
            "fixture approval path available; destructive approvals require second confirmation"
        } else {
            "approval capability unavailable; approval controls disabled until backend supports them"
        },
    );

    println!();
    println!(
        "Next: run `avu doctor --backend {}` and `avu tui --backend fake --once`.",
        adapter.label()
    );
    Ok(())
}

fn print_step(number: usize, name: &str, ok: bool, detail: &str) {
    println!(
        "{}. [{}] {} — {}",
        number,
        if ok { "ok" } else { "!!" },
        name,
        detail
    );
}
