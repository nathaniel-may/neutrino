use std::collections::HashMap;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::Context;
use serde::Serialize;

use crate::config::Config;
use crate::vm;

pub fn install(vm_name: &str) -> anyhow::Result<()> {
    if is_installed(vm_name)? {
        println!("Claude Code already installed, skipping.");
        return Ok(());
    }

    println!("Installing Node.js...");
    vm::run_as_root(vm_name, &["apt-get", "update", "-q"])?;
    vm::run_as_root(vm_name, &["apt-get", "install", "-y", "-q", "nodejs", "npm"])?;

    println!("Installing Claude Code...");
    vm::run_as_root(vm_name, &["npm", "install", "-g", "@anthropic-ai/claude-code"])?;

    println!("Agent installed.");
    Ok(())
}

pub fn write_settings(vm_name: &str, config: &Config) -> anyhow::Result<()> {
    let secrets = match &config.secrets {
        Some(s) => load_secrets(&s.source)?,
        None => HashMap::new(),
    };
    let json = build_settings(config, &secrets)?;

    vm::run(vm_name, &["sh", "-c", "mkdir -p ~/.claude"])?;

    let tmp = std::env::temp_dir().join("neutrino-settings.json");
    std::fs::write(&tmp, &json)?;
    let result = vm::push_file(vm_name, &tmp, ".claude/settings.json");
    std::fs::remove_file(&tmp).ok();
    result?;

    println!("Settings written.");
    Ok(())
}

fn is_installed(vm_name: &str) -> anyhow::Result<bool> {
    let status = Command::new("orb")
        .args(["run", "-m", vm_name, "-u", "root", "which", "claude"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to check agent installation — is the VM running?")?;
    Ok(status.success())
}

pub fn build_settings(config: &Config, secrets: &HashMap<String, String>) -> anyhow::Result<String> {
    let mcp_servers = config
        .mcp_servers
        .iter()
        .map(|mcp| {
            let env = resolve_env(&mcp.env, secrets)?;
            Ok((
                mcp.name.clone(),
                McpServer {
                    command: mcp.command.clone(),
                    args: mcp.args.clone(),
                    env,
                },
            ))
        })
        .collect::<anyhow::Result<HashMap<_, _>>>()?;

    let settings = Settings {
        permissions: Permissions {
            deny: vec!["Bash".into()],
        },
        mcp_servers,
    };

    serde_json::to_string_pretty(&settings).context("failed to serialize settings")
}

pub fn resolve_env(
    env: &HashMap<String, String>,
    secrets: &HashMap<String, String>,
) -> anyhow::Result<HashMap<String, String>> {
    env.iter()
        .map(|(k, v)| {
            if let Some(var_name) = v.strip_prefix('$') {
                let resolved = secrets.get(var_name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "'${}' referenced in mcp env but not found in secrets file",
                        var_name
                    )
                })?;
                Ok((k.clone(), resolved.clone()))
            } else {
                Ok((k.clone(), v.clone()))
            }
        })
        .collect()
}

pub fn parse_env_file(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim().starts_with('#'))
        .filter_map(|l| {
            let (key, value) = l.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn load_secrets(path: &Path) -> anyhow::Result<HashMap<String, String>> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read secrets file: {}", path.display()))?;
    Ok(parse_env_file(&content))
}

#[derive(Serialize)]
struct Settings {
    permissions: Permissions,
    #[serde(rename = "mcpServers", skip_serializing_if = "HashMap::is_empty")]
    mcp_servers: HashMap<String, McpServer>,
}

#[derive(Serialize)]
struct Permissions {
    deny: Vec<String>,
}

#[derive(Serialize)]
struct McpServer {
    command: String,
    args: Vec<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    env: HashMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentConfig, AgentType, VmConfig};

    fn minimal_config() -> Config {
        Config {
            agent: AgentConfig { agent_type: AgentType::Claude },
            vm: VmConfig {
                name: "test".into(),
                distro: "ubuntu:24.04".into(),
                memory_gb: 4,
                cpus: 2,
            },
            secrets: None,
            mcp_servers: vec![],
        }
    }

    #[test]
    fn parse_env_file_basic() {
        let result = parse_env_file("FOO=bar\nBAZ=qux");
        assert_eq!(result["FOO"], "bar");
        assert_eq!(result["BAZ"], "qux");
    }

    #[test]
    fn parse_env_file_ignores_comments_and_blanks() {
        let result = parse_env_file("# comment\n\nFOO=bar\n  # indented\nBAZ=qux");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn resolve_env_passes_through_literals() {
        let env = HashMap::from([("KEY".into(), "literal".into())]);
        let result = resolve_env(&env, &HashMap::new()).unwrap();
        assert_eq!(result["KEY"], "literal");
    }

    #[test]
    fn resolve_env_resolves_dollar_refs() {
        let env = HashMap::from([("KEY".into(), "$MY_SECRET".into())]);
        let secrets = HashMap::from([("MY_SECRET".into(), "resolved".into())]);
        let result = resolve_env(&env, &secrets).unwrap();
        assert_eq!(result["KEY"], "resolved");
    }

    #[test]
    fn resolve_env_errors_on_missing_secret() {
        let env = HashMap::from([("KEY".into(), "$MISSING".into())]);
        assert!(resolve_env(&env, &HashMap::new()).is_err());
    }

    #[test]
    fn build_settings_denies_bash() {
        let json = build_settings(&minimal_config(), &HashMap::new()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v["permissions"]["deny"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("Bash")));
    }

    #[test]
    fn build_settings_omits_mcp_servers_when_empty() {
        let json = build_settings(&minimal_config(), &HashMap::new()).unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("mcpServers").is_none());
    }
}
