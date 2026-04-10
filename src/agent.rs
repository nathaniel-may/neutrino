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
    vm::run_as_root(
        vm_name,
        &["apt-get", "install", "-y", "-q", "nodejs", "npm"],
    )?;

    println!("Installing Claude Code...");
    vm::run_as_root(
        vm_name,
        &["npm", "install", "-g", "@anthropic-ai/claude-code"],
    )?;

    println!("Agent installed.");
    Ok(())
}

pub fn write_settings(config: &Config) -> anyhow::Result<()> {
    let vm_name = &config.vm.name;
    let secrets = match &config.secrets {
        Some(s) => load_secrets(&s.source)?,
        None => HashMap::new(),
    };

    write_permissions(vm_name)?;
    init_claude_config(vm_name)?;
    register_mcp_servers(config, vm_name, &secrets)?;
    lock_settings(vm_name)?;

    println!("Settings written.");
    Ok(())
}

fn lock_settings(vm_name: &str) -> anyhow::Result<()> {
    // Chown settings.json to root so Claude cannot modify its own permissions
    // using its Write tool. Bash is denied, so sudo is unavailable to Claude.
    // ~/.claude.json is left user-owned — Claude needs write access there for
    // session history and project state.
    vm::run(
        vm_name,
        &[
            "sh",
            "-c",
            "sudo chown root:root ~/.claude/settings.json",
        ],
    )
}

fn init_claude_config(vm_name: &str) -> anyhow::Result<()> {
    // Run claude once non-interactively so it initializes ~/.claude.json before
    // we register MCP servers. Without this, Claude overwrites the file on first
    // startup and wipes our registrations.
    let initialized = vm::capture(
        vm_name,
        &["sh", "-c", "test -f ~/.claude.json && echo yes || true"],
    )?;
    if initialized == "yes" {
        return Ok(());
    }
    vm::run(vm_name, &["claude", "--version"])?;
    Ok(())
}

fn write_permissions(vm_name: &str) -> anyhow::Result<()> {
    let json = build_settings()?;
    vm::run(vm_name, &["sh", "-c", "mkdir -p ~/.claude"])?;
    let tmp = std::env::temp_dir().join("neutrino-settings.json");
    std::fs::write(&tmp, &json)?;
    let result = vm::push_file(vm_name, &tmp, ".claude/settings.json");
    std::fs::remove_file(&tmp).ok();
    result
}

fn register_mcp_servers(
    config: &Config,
    vm_name: &str,
    secrets: &HashMap<String, String>,
) -> anyhow::Result<()> {
    // Remove project-level mcpServers overrides so user-scope servers are visible.
    // Claude writes an empty mcpServers:{} for the home project which shadows
    // the user-scoped registrations we add via `claude mcp add -s user`.
    vm::run(
        vm_name,
        &[
            "python3",
            "-c",
            r#"
import json, os
path = os.path.expanduser("~/.claude.json")
if not os.path.exists(path):
    exit(0)
d = json.load(open(path))
for proj in d.get("projects", {}).values():
    proj.pop("mcpServers", None)
json.dump(d, open(path, "w"))
"#,
        ],
    )?;

    for mcp in &config.mcp_servers {
        let env = resolve_env(&mcp.env, secrets)?;

        // Remove first so we always write fresh values (claude mcp add won't
        // update an existing entry).
        let remove_refs = ["claude", "mcp", "remove", &mcp.name];
        let _ = vm::run(vm_name, &remove_refs); // ignore error if not present

        // Build: claude mcp add -s user [-e KEY=VAL ...] -- <name> <command> [args...]
        // The `--` terminates option parsing so the server name is never
        // consumed as an extra value for the variadic -e flag.
        let mut cmd = vec![
            "claude".to_string(),
            "mcp".to_string(),
            "add".to_string(),
            "-s".to_string(),
            "user".to_string(),
        ];
        for (k, v) in &env {
            cmd.push("-e".to_string());
            cmd.push(format!("{k}={v}"));
        }
        cmd.push("--".to_string());
        cmd.push(mcp.name.clone());
        cmd.push(mcp.command.clone());
        cmd.extend(mcp.args.iter().cloned());

        let cmd_refs: Vec<&str> = cmd.iter().map(String::as_str).collect();
        vm::run(vm_name, &cmd_refs)?;
    }
    Ok(())
}

fn is_installed(vm_name: &str) -> anyhow::Result<bool> {
    let status = Command::new("limactl")
        .args(["shell", vm_name, "--", "which", "claude"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to check agent installation — is Lima installed?")?;
    Ok(status.success())
}

pub fn build_settings() -> anyhow::Result<String> {
    let settings = Settings {
        permissions: Permissions {
            deny: vec!["Bash".into()],
        },
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
}

#[derive(Serialize)]
struct Permissions {
    deny: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentConfig, AgentType, VmConfig};

    fn minimal_config() -> Config {
        Config {
            agent: AgentConfig {
                agent_type: AgentType::Claude,
            },
            vm: VmConfig {
                name: "test".into(),
                distro: "ubuntu:24.04".into(),
                memory_gb: 4,
                cpus: 2,
            },
            setup: None,
            attach: None,
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
        let json = build_settings().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(
            v["permissions"]["deny"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("Bash"))
        );
    }

    #[test]
    fn build_settings_omits_mcp_servers() {
        let json = build_settings().unwrap();
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(v.get("mcpServers").is_none());
    }
}
