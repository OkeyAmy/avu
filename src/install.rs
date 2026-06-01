use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::InstallArgs;
use crate::config::AvuPaths;

#[derive(Debug, Serialize)]
struct InstallReport {
    ok: bool,
    os: String,
    arch: String,
    avu_bin: String,
    avu_home: String,
    install_method: InstallMethod,
    path_ok: bool,
    backends: Vec<BackendDetect>,
    checks: Vec<InstallCheck>,
}

#[derive(Debug, Serialize)]
enum InstallMethod {
    Script,
    PackageManager,
    Cargo,
    Source,
    Unknown,
}

#[derive(Debug, Serialize)]
struct BackendDetect {
    name: String,
    found: bool,
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct InstallCheck {
    name: &'static str,
    ok: bool,
    detail: String,
}

pub fn run(args: InstallArgs) -> Result<()> {
    let paths = AvuPaths::discover();
    let report = build_report(&paths)?;

    if args.check {
        print_check(&report);
    } else if args.fix_self {
        fix_path(&paths)?;
    } else if args.info {
        print_info(&report);
    } else {
        print_full(&report);
    }

    Ok(())
}

fn build_report(paths: &AvuPaths) -> Result<InstallReport> {
    let os = detect_os();
    let arch = detect_arch();
    let avu_bin = env::current_exe()
        .context("cannot determine current executable path")?
        .display()
        .to_string();
    let install_method = detect_install_method(&avu_bin);
    let path_ok = is_on_path(&avu_bin);
    let backends = detect_backends();

    let mut checks = Vec::new();

    checks.push(InstallCheck {
        name: "avu_binary_exists",
        ok: Path::new(&avu_bin).exists(),
        detail: avu_bin.clone(),
    });

    checks.push(InstallCheck {
        name: "avu_on_path",
        ok: path_ok,
        detail: if path_ok {
            "avu is callable from PATH".to_string()
        } else {
            "avu is NOT on PATH; run 'avu install --fix-self' or add to PATH manually".to_string()
        },
    });

    checks.push(InstallCheck {
        name: "avu_home_exists",
        ok: paths.home.exists(),
        detail: paths.home.display().to_string(),
    });

    let any_backend = backends.iter().any(|b| b.found);
    checks.push(InstallCheck {
        name: "backend_detected",
        ok: true,
        detail: if any_backend {
            format!(
                "found: {}",
                backends
                    .iter()
                    .filter(|b| b.found)
                    .map(|b| b.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            "no agent backend found; install is still valid, but setup will need Hermes, OpenClaw, or a remote backend".to_string()
        },
    });

    checks.push(InstallCheck {
        name: "install_method",
        ok: true,
        detail: format!("{:?}", install_method).to_lowercase(),
    });

    let ok = checks.iter().all(|c| c.ok);

    Ok(InstallReport {
        ok,
        os,
        arch,
        avu_bin,
        avu_home: paths.home.display().to_string(),
        install_method,
        path_ok,
        backends,
        checks,
    })
}

fn detect_os() -> String {
    if cfg!(target_os = "linux") {
        if cfg!(target_env = "musl") {
            "linux-musl".to_string()
        } else {
            "linux-gnu".to_string()
        }
    } else if cfg!(target_os = "macos") {
        "macos".to_string()
    } else if cfg!(target_os = "windows") {
        "windows".to_string()
    } else {
        "unknown".to_string()
    }
}

fn detect_arch() -> String {
    if cfg!(target_arch = "x86_64") {
        "x86_64".to_string()
    } else if cfg!(target_arch = "aarch64") {
        "aarch64".to_string()
    } else {
        "unknown".to_string()
    }
}

fn detect_install_method(avu_bin: &str) -> InstallMethod {
    let path = Path::new(avu_bin);
    let path_text = path.to_string_lossy();

    if path_text.contains("target/debug") || path_text.contains("target/release") {
        InstallMethod::Source
    } else if path_text.contains(".cargo") {
        InstallMethod::Cargo
    } else if path.starts_with("/usr/") || path.starts_with("C:\\Program") {
        InstallMethod::PackageManager
    } else if path.starts_with(env::var("HOME").unwrap_or_default())
        || path.starts_with("/home/")
        || path.starts_with("/Users/")
        || path.starts_with("C:\\Users\\")
    {
        InstallMethod::Script
    } else {
        InstallMethod::Unknown
    }
}

fn is_on_path(avu_bin: &str) -> bool {
    let bin_name = Path::new(avu_bin)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    which_path(&bin_name)
        .map(|found| found == Path::new(avu_bin))
        .unwrap_or(false)
}

fn which_path(name: &str) -> Option<PathBuf> {
    let path_var = env::var("PATH").ok()?;
    let separator = if cfg!(windows) { ';' } else { ':' };

    for dir in path_var.split(separator) {
        let candidate = PathBuf::from(dir).join(name);
        if candidate.exists() {
            return Some(candidate);
        }
        if cfg!(windows) {
            let with_exe = candidate.with_extension("exe");
            if with_exe.exists() {
                return Some(with_exe);
            }
        }
    }

    None
}

fn detect_backends() -> Vec<BackendDetect> {
    let mut backends = Vec::new();

    for name in &["hermes", "openclaw"] {
        let found = which_path(name).is_some();
        let path = which_path(name).map(|p| p.display().to_string());
        backends.push(BackendDetect {
            name: name.to_string(),
            found,
            path,
        });
    }

    backends
}

fn fix_path(paths: &AvuPaths) -> Result<()> {
    let home = env::var("HOME").context("HOME not set")?;
    let local_bin = PathBuf::from(&home).join(".local").join("bin");

    if !local_bin.exists() {
        std::fs::create_dir_all(&local_bin)
            .with_context(|| format!("cannot create {}", local_bin.display()))?;
        println!("Created {}", local_bin.display());
    }

    let avu_exe = env::current_exe().context("cannot find current avu binary")?;
    let dest = local_bin.join(
        avu_exe
            .file_name()
            .context("no filename in current exe path")?,
    );

    if dest.exists() {
        println!("avu already exists at {}", dest.display());
    } else {
        std::fs::copy(&avu_exe, &dest)
            .with_context(|| format!("cannot copy {:?} to {:?}", avu_exe, dest))?;
        println!("Copied avu to {}", dest.display());
    }

    let path_var = env::var("PATH").unwrap_or_default();
    let separator = if cfg!(windows) { ';' } else { ':' };

    if !path_var
        .split(separator)
        .any(|p| p == local_bin.to_string_lossy())
    {
        let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let shell_name = Path::new(&shell)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        println!();
        println!(
            "{} is NOT on PATH. Add it permanently:",
            local_bin.display()
        );
        println!();

        match shell_name.as_ref() {
            "zsh" => {
                println!(
                    "  echo 'export PATH=\"{}:$PATH\"' >> ~/.zshrc",
                    local_bin.display()
                );
                println!("  source ~/.zshrc");
            }
            "fish" => {
                println!("  fish_add_path {}", local_bin.display());
            }
            _ => {
                println!(
                    "  echo 'export PATH=\"{}:$PATH\"' >> ~/.bashrc",
                    local_bin.display()
                );
                println!("  source ~/.bashrc");
            }
        }

        let new_path = format!("{}{}{}", local_bin.display(), separator, path_var);
        // SAFETY: This is a single-threaded CLI tool; no other threads
        // are reading PATH concurrently at this point.
        unsafe {
            env::set_var("PATH", &new_path);
        }
        println!();
        println!("PATH updated for this session. Open a new terminal for it to persist.");
    } else {
        println!("{} is already on PATH.", local_bin.display());
    }

    if !paths.home.exists() {
        std::fs::create_dir_all(&paths.home)
            .with_context(|| format!("cannot create {}", paths.home.display()))?;
        std::fs::create_dir_all(paths.home.join("logs"))
            .with_context(|| "cannot create logs dir")?;
        std::fs::create_dir_all(paths.home.join("fixtures"))
            .with_context(|| "cannot create fixtures dir")?;
        println!("Created {} directory structure.", paths.home.display());
    }

    Ok(())
}

fn print_full(report: &InstallReport) {
    println!("Avu install");
    println!("===========");
    println!("OS:              {}", report.os);
    println!("Architecture:    {}", report.arch);
    println!("Avu binary:      {}", report.avu_bin);
    println!("Avu home:        {}", report.avu_home);
    println!("Install method:  {:?}", report.install_method);
    println!(
        "On PATH:         {}",
        if report.path_ok { "yes" } else { "no" }
    );
    println!();

    println!("Backends:");
    for b in &report.backends {
        println!(
            "  [{}] {} {}",
            if b.found { "ok" } else { "  " },
            b.name,
            b.path.as_deref().unwrap_or("not found")
        );
    }
    println!();

    for check in &report.checks {
        println!(
            "[{}] {} — {}",
            if check.ok { "ok" } else { "!!" },
            check.name,
            check.detail
        );
    }

    println!();
    if report.ok {
        println!("All checks passed.");
    } else {
        println!("Some checks need attention. Run 'avu install --check' for details.");
    }
}

fn print_check(report: &InstallReport) {
    for check in &report.checks {
        println!(
            "[{}] {} — {}",
            if check.ok { "ok" } else { "FAIL" },
            check.name,
            check.detail
        );
    }

    if !report.ok {
        std::process::exit(1);
    }
}

fn print_info(report: &InstallReport) {
    println!(
        "{}",
        serde_json::to_string_pretty(report)
            .unwrap_or_else(|e| format!("{{\"error\": \"{}\"}}", e))
    );
}
