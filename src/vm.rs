use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, bail};
use toml;

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

pub fn check_drift(config: &VmConfig) -> anyhow::Result<()> {
    if !exists(&config.name)? {
        return Ok(());
    }
    let stored_toml = capture(
        &config.name,
        &["sh", "-c", "cat ~/.neutrino-vm.toml 2>/dev/null || true"],
    )?;
    if stored_toml.is_empty() {
        return Ok(());
    }
    let stored: VmConfig =
        toml::from_str(&stored_toml).context("failed to parse stored VM config")?;
    if let Some(msg) = drift_message(&stored, config) {
        anyhow::bail!("{}", msg);
    }
    Ok(())
}

pub fn save_config(config: &VmConfig) -> anyhow::Result<()> {
    let already_saved = capture(
        &config.name,
        &[
            "sh",
            "-c",
            "test -f ~/.neutrino-vm.toml && echo yes || true",
        ],
    )?;
    if already_saved == "yes" {
        return Ok(());
    }
    let toml = toml::to_string(config).context("failed to serialize VM config")?;
    let tmp = std::env::temp_dir().join("neutrino-vm.toml");
    std::fs::write(&tmp, toml)?;
    let result = push_file(&config.name, &tmp, ".neutrino-vm.toml");
    std::fs::remove_file(&tmp).ok();
    result
}

fn drift_message(stored: &VmConfig, current: &VmConfig) -> Option<String> {
    if stored == current {
        return None;
    }
    let mut changes = vec![];
    if stored.name != current.name {
        changes.push(format!(
            "  name:      {:?} → {:?}",
            stored.name, current.name
        ));
    }
    if stored.distro != current.distro {
        changes.push(format!(
            "  distro:    {:?} → {:?}",
            stored.distro, current.distro
        ));
    }
    if stored.memory_gb != current.memory_gb {
        changes.push(format!(
            "  memory_gb: {} → {}",
            stored.memory_gb, current.memory_gb
        ));
    }
    if stored.cpus != current.cpus {
        changes.push(format!("  cpus:      {} → {}", stored.cpus, current.cpus));
    }
    Some(format!(
        "VM '{}' was created with a different configuration:\n{}\nRun `neutrino down` then re-run to apply VM changes.",
        current.name,
        changes.join("\n"),
    ))
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

    #[test]
    fn drift_message_none_when_configs_match() {
        assert!(drift_message(&test_config(), &test_config()).is_none());
    }

    #[test]
    fn drift_message_some_when_distro_changes() {
        let mut updated = test_config();
        updated.distro = "ubuntu:22.04".into();
        let msg = drift_message(&test_config(), &updated).unwrap();
        assert!(msg.contains("distro"));
        assert!(msg.contains("ubuntu:24.04"));
        assert!(msg.contains("ubuntu:22.04"));
        assert!(msg.contains("neutrino down"));
    }

    #[test]
    fn drift_message_reports_all_changed_fields() {
        let mut updated = test_config();
        updated.distro = "debian:12".into();
        updated.memory_gb = 8;
        let msg = drift_message(&test_config(), &updated).unwrap();
        assert!(msg.contains("distro"));
        assert!(msg.contains("memory_gb"));
    }
}
