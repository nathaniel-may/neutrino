use std::io::Write as IoWrite;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

use shellexpand;

use anyhow::{Context, bail};

use crate::config::VmConfig;

pub fn up(config: &VmConfig) -> anyhow::Result<()> {
    if exists(&config.name)? {
        println!("Starting '{}'...", config.name);
        run_lima(&["start", &config.name])?;
    } else {
        println!("Creating '{}'...", config.name);
        let yaml = lima_yaml(config)?;
        let tmp = std::env::temp_dir().join(format!("neutrino-{}.yaml", config.name));
        std::fs::write(&tmp, &yaml)?;
        let result = run_lima(&["create", "--name", &config.name, tmp.to_str().unwrap()]);
        std::fs::remove_file(&tmp).ok();
        result?;
        run_lima(&["start", &config.name])?;
    }
    write_ssh_config(&config.name)?;
    println!(
        "  {} CPUs, {}GB memory (adjust in .neutrino.toml)",
        config.cpus, config.memory_gb
    );
    Ok(())
}

fn write_ssh_config(name: &str) -> anyhow::Result<()> {
    let output = Command::new("limactl")
        .args(["show-ssh", "--format", "config", name])
        .output()
        .context("failed to get Lima SSH config")?;
    if !output.status.success() {
        return Ok(());
    }
    let path = shellexpand::tilde("~/.lima/_config/ssh.config").into_owned();
    std::fs::write(&path, &output.stdout)
        .with_context(|| format!("failed to write SSH config to {path}"))?;
    Ok(())
}

pub fn down(config: &VmConfig) -> anyhow::Result<()> {
    if !exists(&config.name)? {
        bail!("VM '{}' does not exist", config.name);
    }
    println!("Deleting '{}'...", config.name);
    let _ = run_lima(&["stop", &config.name]);
    run_lima(&["delete", &config.name])?;
    Ok(())
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

/// Replace the current process with an interactive command in the VM.
/// Only returns if exec fails.
pub fn exec(vm_name: &str, cmd: &[&str]) -> anyhow::Error {
    let mut args = vec!["shell", vm_name];
    if !cmd.is_empty() {
        args.push("--");
        args.extend(cmd.iter().copied());
    }
    let full_cmd = format!("limactl {}", args.join(" "));
    let err = Command::new("limactl").args(&args).exec();
    anyhow::Error::from(err).context(format!("failed to exec '{full_cmd}'"))
}

pub fn run(vm_name: &str, cmd: &[&str]) -> anyhow::Result<()> {
    let mut args = vec!["shell", vm_name, "--"];
    args.extend(cmd.iter().copied());
    let status = Command::new("limactl")
        .args(&args)
        .status()
        .with_context(|| format!("failed to run '{}' in VM '{}'", cmd.join(" "), vm_name))?;
    if !status.success() {
        bail!("'{}' failed in VM '{}'", cmd.join(" "), vm_name);
    }
    Ok(())
}

pub fn run_as_root(vm_name: &str, cmd: &[&str]) -> anyhow::Result<()> {
    let mut args = vec!["shell", vm_name, "--", "sudo"];
    args.extend(cmd.iter().copied());
    let status = Command::new("limactl")
        .args(&args)
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
    let mut args = vec!["shell", vm_name, "--"];
    args.extend(cmd.iter().copied());
    let output = Command::new("limactl")
        .args(&args)
        .output()
        .with_context(|| format!("failed to run '{}' in VM '{}'", cmd.join(" "), vm_name))?;
    if !output.status.success() {
        bail!("'{}' failed in VM '{}'", cmd.join(" "), vm_name);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn push_file(vm_name: &str, src: &Path, dest: &str) -> anyhow::Result<()> {
    let content =
        std::fs::read(src).with_context(|| format!("failed to read {}", src.display()))?;
    let script = format!("mkdir -p $(dirname ~/{dest}) && cat > ~/{dest}");
    let mut child = Command::new("limactl")
        .args(["shell", vm_name, "--", "sh", "-c", &script])
        .stdin(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to push {} to VM '{}'", src.display(), vm_name))?;
    child.stdin.as_mut().unwrap().write_all(&content)?;
    let status = child.wait()?;
    if !status.success() {
        bail!("failed to push {} to VM '{}'", src.display(), vm_name);
    }
    Ok(())
}

fn exists(name: &str) -> anyhow::Result<bool> {
    let status = Command::new("limactl")
        .args(["list", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to run 'limactl list' — is Lima installed?")?;
    Ok(status.success())
}

fn run_lima(args: &[&str]) -> anyhow::Result<()> {
    let status = Command::new("limactl")
        .args(args)
        .status()
        .with_context(|| format!("failed to run 'limactl {}'", args.join(" ")))?;
    if !status.success() {
        bail!("'limactl {}' exited with {}", args.join(" "), status);
    }
    Ok(())
}

fn lima_yaml(config: &VmConfig) -> anyhow::Result<String> {
    let (arm64, amd64) = match config.distro.as_str() {
        "ubuntu:24.04" => (
            "https://cloud-images.ubuntu.com/releases/24.04/release/ubuntu-24.04-server-cloudimg-arm64.img",
            "https://cloud-images.ubuntu.com/releases/24.04/release/ubuntu-24.04-server-cloudimg-amd64.img",
        ),
        "ubuntu:22.04" => (
            "https://cloud-images.ubuntu.com/releases/22.04/release/ubuntu-22.04-server-cloudimg-arm64.img",
            "https://cloud-images.ubuntu.com/releases/22.04/release/ubuntu-22.04-server-cloudimg-amd64.img",
        ),
        other => bail!(
            "unsupported distro '{}' — supported: ubuntu:24.04, ubuntu:22.04",
            other
        ),
    };
    Ok(format!(
        "cpus: {cpus}\nmemory: \"{memory}GiB\"\nmounts: []\nimages:\n  - location: \"{arm64}\"\n    arch: \"aarch64\"\n  - location: \"{amd64}\"\n    arch: \"x86_64\"\n",
        cpus = config.cpus,
        memory = config.memory_gb,
    ))
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
    fn lima_yaml_contains_resources() {
        let yaml = lima_yaml(&test_config()).unwrap();
        assert!(yaml.contains("cpus: 2"));
        assert!(yaml.contains("memory: \"4GiB\""));
        assert!(yaml.contains("mounts: []"));
    }

    #[test]
    fn lima_yaml_ubuntu_24_04_has_both_arches() {
        let yaml = lima_yaml(&test_config()).unwrap();
        assert!(yaml.contains("aarch64"));
        assert!(yaml.contains("x86_64"));
        assert!(yaml.contains("24.04"));
    }

    #[test]
    fn lima_yaml_unsupported_distro_errors() {
        let mut config = test_config();
        config.distro = "fedora:40".into();
        assert!(lima_yaml(&config).is_err());
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
        updated.distro = "ubuntu:22.04".into();
        updated.memory_gb = 8;
        let msg = drift_message(&test_config(), &updated).unwrap();
        assert!(msg.contains("distro"));
        assert!(msg.contains("memory_gb"));
    }
}
