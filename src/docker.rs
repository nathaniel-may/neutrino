use std::process::{Command, Stdio};

use anyhow::Context;

use crate::config::Config;
use crate::vm;

pub fn is_needed(config: &Config) -> bool {
    config.mcp_servers.iter().any(|m| m.command == "docker")
}

pub fn install_if_needed(config: &Config) -> anyhow::Result<()> {
    if !is_needed(config) {
        return Ok(());
    }
    let vm_name = &config.vm.name;
    if is_installed(vm_name)? {
        println!("Docker already installed, skipping.");
        return Ok(());
    }
    install(vm_name)
}

fn is_installed(vm_name: &str) -> anyhow::Result<bool> {
    let status = Command::new("orb")
        .args(["run", "-m", vm_name, "which", "docker"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to check Docker installation")?;
    Ok(status.success())
}

fn install(vm_name: &str) -> anyhow::Result<()> {
    println!("Installing Docker...");

    vm::run_as_root(
        vm_name,
        &["apt-get", "install", "-y", "-q", "ca-certificates", "curl"],
    )?;

    vm::run_as_root(
        vm_name,
        &["install", "-m", "0755", "-d", "/etc/apt/keyrings"],
    )?;

    vm::run_as_root(
        vm_name,
        &[
            "sh",
            "-c",
            "curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc \
         && chmod a+r /etc/apt/keyrings/docker.asc",
        ],
    )?;

    vm::run_as_root(
        vm_name,
        &[
            "sh",
            "-c",
            "echo \"deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] \
         https://download.docker.com/linux/ubuntu \
         $(. /etc/os-release && echo \"${UBUNTU_CODENAME:-$VERSION_CODENAME}\") stable\" \
         | tee /etc/apt/sources.list.d/docker.list > /dev/null",
        ],
    )?;

    vm::run_as_root(vm_name, &["apt-get", "update", "-q"])?;
    vm::run_as_root(
        vm_name,
        &[
            "apt-get",
            "install",
            "-y",
            "-q",
            "docker-ce",
            "docker-ce-cli",
            "containerd.io",
            "docker-buildx-plugin",
            "docker-compose-plugin",
        ],
    )?;

    vm::run_as_root(vm_name, &["systemctl", "enable", "docker"])?;
    vm::run_as_root(vm_name, &["systemctl", "start", "docker"])?;

    let user = vm::capture(vm_name, &["whoami"])?;
    vm::run_as_root(vm_name, &["usermod", "-aG", "docker", &user])?;

    println!("Docker installed.");
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
    fn is_needed_false_when_mcp_uses_npx() {
        let mut config = minimal_config();
        config.mcp_servers.push(McpConfig {
            name: "some-server".into(),
            command: "npx".into(),
            args: vec![],
            env: HashMap::new(),
        });
        assert!(!is_needed(&config));
    }

    #[test]
    fn is_needed_true_when_mcp_uses_docker() {
        let mut config = minimal_config();
        config.mcp_servers.push(McpConfig {
            name: "github".into(),
            command: "docker".into(),
            args: vec!["run".into(), "ghcr.io/github/github-mcp-server".into()],
            env: HashMap::new(),
        });
        assert!(is_needed(&config));
    }
}
