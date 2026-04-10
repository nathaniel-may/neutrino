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
    let status = Command::new("limactl")
        .args(["shell", vm_name, "--", "test", "-f", "/usr/local/bin/uvx"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to check uv installation")?;
    Ok(status.success())
}

fn install(vm_name: &str) -> anyhow::Result<()> {
    println!("Installing uv...");
    vm::run_as_root(vm_name, &["apt-get", "install", "-y", "-q", "curl"])?;
    // UV_INSTALL_DIR places uv/uvx directly in /usr/local/bin — no intermediate
    // home directory copy, no symlinks, no PATH shadowing warnings.
    vm::run_as_root(
        vm_name,
        &[
            "sh",
            "-c",
            "curl -LsSf https://astral.sh/uv/install.sh | UV_INSTALL_DIR=/usr/local/bin sh",
        ],
    )?;
    println!("uv installed.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{McpConfig, minimal_config};
    use std::collections::HashMap;

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
