use crate::cli::ConfigArgs;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{env, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct AvuPaths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub auth: PathBuf,
    pub capabilities: PathBuf,
    pub logs: PathBuf,
    pub fixtures: PathBuf,
}

impl AvuPaths {
    pub fn discover() -> Self {
        let home = env::var_os("AVU_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".avu")))
            .unwrap_or_else(|| PathBuf::from(".avu"));

        Self {
            config: home.join("config.toml"),
            auth: home.join("auth.json"),
            capabilities: home.join("capabilities.json"),
            logs: home.join("logs"),
            fixtures: home.join("fixtures"),
            home,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub struct AvuConfig {
    pub backend: String,
    pub permission_posture: String,
    pub wake_mode: String,
    pub wake_phrase: String,
}

impl Default for AvuConfig {
    fn default() -> Self {
        Self {
            backend: "auto".to_string(),
            permission_posture: "observe_notify".to_string(),
            wake_mode: "keyboard_only".to_string(),
            wake_phrase: "hey avu".to_string(),
        }
    }
}

impl AvuConfig {
    pub fn load(paths: &AvuPaths) -> Result<Self> {
        match std::fs::read_to_string(&paths.config) {
            Ok(contents) => Ok(toml::from_str(&contents)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error.into()),
        }
    }
}

pub fn print_effective(args: ConfigArgs) -> Result<()> {
    let paths = AvuPaths::discover();
    let config = AvuConfig::load(&paths)?;
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "paths": paths, "config": config }))?
        );
    } else {
        println!("Avu config home: {}", paths.home.display());
        println!("Config: {}", paths.config.display());
        println!("Auth metadata: {}", paths.auth.display());
        println!("Capability snapshot: {}", paths.capabilities.display());
        println!("Logs: {}", paths.logs.display());
        println!("Fixtures: {}", paths.fixtures.display());
        println!("Default backend: {}", config.backend);
        println!("Default wake mode: {}", config.wake_mode);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_config_from_toml_text() {
        let input = r#"
backend = "hermes"
permission_posture = "observe_notify"
wake_mode = "keyboard_only"
wake_phrase = "hey avu"
"#;
        let config: AvuConfig = toml::from_str(input).expect("valid config");
        assert_eq!(config.backend, "hermes");
        assert_eq!(config.wake_phrase, "hey avu");
    }
}
