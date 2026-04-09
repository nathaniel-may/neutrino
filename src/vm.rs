use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};

use crate::config::VmConfig;

pub fn up(config: &VmConfig) -> anyhow::Result<()> {
    if exists(&config.name)? {
        println!("Starting '{}'...", config.name);
        run_orb(&start_args(&config.name))?;
    } else {
        println!("Creating '{}'...", config.name);
        run_orb(&create_args(config))?;
    }
    print_orb_resources();
    Ok(())
}

fn print_orb_resources() {
    if let (Ok(cpus), Ok(memory_mib)) = (orb_config_get("cpu"), orb_config_get("memory_mib")) {
        println!(
            "OrbStack resources: {} CPUs, {}GB memory",
            cpus,
            memory_mib / 1024
        );
        println!("To adjust: OrbStack menu → Preferences → Resources (restart VM to apply)");
    }
}

fn orb_config_get(key: &str) -> anyhow::Result<u32> {
    let output = Command::new("orb")
        .args(["config", "get", key])
        .output()
        .context("failed to run 'orb config get'")?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .context("failed to parse orb config value")
}

pub fn down(config: &VmConfig) -> anyhow::Result<()> {
    if !exists(&config.name)? {
        bail!("VM '{}' does not exist", config.name);
    }
    println!("Deleting '{}'...", config.name);
    run_orb(&delete_args(&config.name))?;
    Ok(())
}

fn exists(name: &str) -> anyhow::Result<bool> {
    let status = Command::new("orb")
        .args(["info", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run 'orb info' — is OrbStack installed?")?;
    Ok(status.success())
}

fn run_orb(args: &[String]) -> anyhow::Result<()> {
    let status = Command::new("orb")
        .args(args)
        .status()
        .with_context(|| format!("failed to run 'orb {}'", args.join(" ")))?;
    if !status.success() {
        bail!("'orb {}' exited with {}", args.join(" "), status);
    }
    Ok(())
}

/// Replace the current process with an interactive command in the VM.
/// Only returns if exec fails.
pub fn exec(vm_name: &str, cmd: &[&str]) -> anyhow::Error {
    anyhow::Error::from(
        Command::new("orb")
            .args(["run", "-m", vm_name])
            .args(cmd)
            .exec(),
    )
}

pub fn run(vm_name: &str, cmd: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("orb")
        .args(["run", "-m", vm_name])
        .args(cmd)
        .status()
        .with_context(|| format!("failed to run '{}' in VM '{}'", cmd.join(" "), vm_name))?;
    if !status.success() {
        bail!("'{}' failed in VM '{}'", cmd.join(" "), vm_name);
    }
    Ok(())
}

pub fn run_as_root(vm_name: &str, cmd: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("orb")
        .args(["run", "-m", vm_name, "-u", "root"])
        .args(cmd)
        .status()
        .with_context(|| {
            format!(
                "failed to run '{}' as root in VM '{}'",
                cmd.join(" "),
                vm_name
            )
        })?;
    if !status.success() {
        bail!("'{}' failed as root in VM '{}'", cmd.join(" "), vm_name);
    }
    Ok(())
}

pub fn capture(vm_name: &str, cmd: &[&str]) -> anyhow::Result<String> {
    let output = Command::new("orb")
        .args(["run", "-m", vm_name])
        .args(cmd)
        .output()
        .with_context(|| format!("failed to run '{}' in VM '{}'", cmd.join(" "), vm_name))?;
    if !output.status.success() {
        bail!("'{}' failed in VM '{}'", cmd.join(" "), vm_name);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn push_file(vm_name: &str, src: &Path, dest: &str) -> anyhow::Result<()> {
    let status = Command::new("orb")
        .args(["push", "-m", vm_name, src.to_str().unwrap(), dest])
        .status()
        .with_context(|| format!("failed to push {} to VM '{}'", src.display(), vm_name))?;
    if !status.success() {
        bail!("failed to push {} to VM '{}'", src.display(), vm_name);
    }
    Ok(())
}

fn create_args(config: &VmConfig) -> Vec<String> {
    // OrbStack does not support per-VM memory or CPU limits via CLI.
    // memory_gb and cpus from config are reserved for future provider support.
    vec!["create".into(), config.distro.clone(), config.name.clone()]
}

fn start_args(name: &str) -> Vec<String> {
    vec!["start".into(), name.into()]
}

fn delete_args(name: &str) -> Vec<String> {
    // --force skips confirmation prompt and stops the VM if running
    vec!["delete".into(), "--force".into(), name.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> VmConfig {
        VmConfig {
            name: "test-vm".into(),
            distro: "ubuntu:24.04".into(),
            memory_gb: 4,
            cpus: 2,
        }
    }

    #[test]
    fn create_args_orders_distro_before_name() {
        let args = create_args(&test_config());
        assert_eq!(args, vec!["create", "ubuntu:24.04", "test-vm"]);
    }

    #[test]
    fn start_args_includes_name() {
        let args = start_args("test-vm");
        assert_eq!(args, vec!["start", "test-vm"]);
    }

    #[test]
    fn delete_args_includes_force_flag() {
        let args = delete_args("test-vm");
        assert_eq!(args, vec!["delete", "--force", "test-vm"]);
    }
}
