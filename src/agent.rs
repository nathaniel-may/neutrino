use std::process::{Command, Stdio};

use anyhow::{bail, Context};

pub fn install(vm_name: &str) -> anyhow::Result<()> {
    if is_installed(vm_name)? {
        println!("Claude Code already installed, skipping.");
        return Ok(());
    }

    println!("Installing Node.js...");
    run_in_vm(vm_name, &["apt-get", "update", "-q"])?;
    run_in_vm(vm_name, &["apt-get", "install", "-y", "-q", "nodejs", "npm"])?;

    println!("Installing Claude Code...");
    run_in_vm(vm_name, &["npm", "install", "-g", "@anthropic-ai/claude-code"])?;

    println!("Agent installed.");
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

fn run_in_vm(vm_name: &str, cmd: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("orb")
        .args(["run", "-m", vm_name, "-u", "root"])
        .args(cmd)
        .status()
        .with_context(|| format!("failed to run '{}' in VM", cmd.join(" ")))?;
    if !status.success() {
        bail!("'{}' failed in VM '{}'", cmd.join(" "), vm_name);
    }
    Ok(())
}
