use std::process::{Command, Stdio};

use anyhow::Context;

use crate::config::Config;
use crate::vm;

pub fn is_needed(config: &Config) -> bool {
    config.mcp_servers.iter().any(|m| m.command == "uvx")
}

pub fn install_if_needed(config: &Config) -> anyhow::Result<()> {
    if !is_needed(config) {
        return Ok(());
    }
    let vm_name = &config.vm.name;
    if is_installed(vm_name)? {
        println!("uv already installed, skipping.");
        return Ok(());
    }
    install(vm_name)
}

fn is_installed(vm_name: &str) -> anyhow::Result<bool> {
    let status = Command::new("orb")
        .args(["run", "-m", vm_name, "which", "uvx"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to check uv installation")?;
    Ok(status.success())
}

fn install(vm_name: &str) -> anyhow::Result<()> {
    println!("Installing uv...");
    vm::run_as_root(vm_name, &["apt-get", "install", "-y", "-q", "curl"])?;
    // Install uv as root, then symlink to /usr/local/bin so it's in PATH for all users.
    vm::run_as_root(
        vm_name,
        &[
            "sh",
            "-c",
            "curl -LsSf https://astral.sh/uv/install.sh | sh \
         && ln -sf /root/.local/bin/uv /usr/local/bin/uv \
         && ln -sf /root/.local/bin/uvx /usr/local/bin/uvx",
        ],
    )?;
    println!("uv installed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AgentConfig, AgentType, McpConfig, VmConfig};
    use std::collections::HashMap;

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
            attach: None,
            secrets: None,
            mcp_servers: vec![],
        }
    }

    #[test]
    fn is_needed_false_when_no_mcp_servers() {
        assert!(!is_needed(&minimal_config()));
    }

    #[test]
    fn is_needed_false_when_mcp_uses_docker() {
        let mut config = minimal_config();
        config.mcp_servers.push(McpConfig {
            name: "github".into(),
            command: "docker".into(),
            args: vec![],
            env: HashMap::new(),
        });
        assert!(!is_needed(&config));
    }

    #[test]
    fn is_needed_true_when_mcp_uses_uvx() {
        let mut config = minimal_config();
        config.mcp_servers.push(McpConfig {
            name: "git".into(),
            command: "uvx".into(),
            args: vec!["mcp-server-git".into()],
            env: HashMap::new(),
        });
        assert!(is_needed(&config));
    }
}
