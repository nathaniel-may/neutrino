mod agent;
mod config;
mod docker;
mod setup;
mod uv;
mod vm;

use clap::{Parser, Subcommand};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command as Process;

#[derive(Parser)]
#[command(name = "neutrino", about = "Sandboxed AI agent environments")]
struct Cli {
    /// Path to the config file
    #[arg(short, long, default_value = ".neutrino.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Generate a default .neutrino.toml in the current directory
    Init,
    /// Validate a .neutrino.toml config file
    Validate,
    /// Provision the VM and launch the agent
    Run,
    /// Stop and delete the VM defined in the config
    Down,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Init => init(),
        Command::Validate => validate(&cli.config),
        Command::Run => {
            let config = config::Config::from_file(&cli.config)?;
            vm::up(&config.vm)?;
            vm::check_drift(&config.vm)?;
            vm::save_config(&config.vm)?;
            agent::install(&config.vm.name)?;
            docker::install_if_needed(&config)?;
            uv::install_if_needed(&config)?;
            setup::run_if_needed(&config)?;
            agent::write_settings(&config)?;
            if let Some(ref attach) = config.attach {
                let args = attach.resolved_args(&config);
                let err = Process::new(&attach.command).args(&args).exec();
                Err(anyhow::Error::from(err)
                    .context(format!("failed to exec '{}'", attach.command)))
            } else if docker::is_needed(&config) {
                Err(vm::exec(&config.vm.name, &["sg", "docker", "-c", "bash"]))
            } else {
                Err(vm::exec(&config.vm.name, &["bash"]))
            }
        }
        Command::Down => {
            let config = config::Config::from_file(&cli.config)?;
            vm::down(&config.vm)
        }
    }
}

fn init() -> anyhow::Result<()> {
    let path = PathBuf::from(".neutrino.toml");
    if path.exists() {
        anyhow::bail!(".neutrino.toml already exists");
    }
    std::fs::write(&path, DEFAULT_CONFIG)?;
    println!("Created .neutrino.toml");
    Ok(())
}

fn validate(config_path: &Path) -> anyhow::Result<()> {
    let config = config::Config::from_file(config_path)?;

    println!("Config is valid.");
    println!("  agent:   {}", config.agent.agent_type);
    println!(
        "  vm:      {} ({}, {}GB, {} CPUs)",
        config.vm.name, config.vm.distro, config.vm.memory_gb, config.vm.cpus
    );
    if let Some(attach) = &config.attach {
        println!(
            "  attach:  {} {}",
            attach.command,
            attach.resolved_args(&config).join(" ")
        );
    }
    if let Some(secrets) = &config.secrets {
        println!("  secrets: {}", secrets.source.display());
    }
    if !config.mcp_servers.is_empty() {
        let names: Vec<&str> = config.mcp_servers.iter().map(|m| m.name.as_str()).collect();
        println!("  mcp:     {}", names.join(", "));
    }

    Ok(())
}

const DEFAULT_CONFIG: &str = r#"[agent]
type = "claude"

[vm]
name = "my-project"
distro = "ubuntu:24.04"
memory_gb = 4
cpus = 2

[setup]
run = [
  "git clone https://github.com/your/repo.git",
]

[attach]
command = "limactl"
args = ["shell", "{config.vm.name}"]

# [secrets]
# source = ".env"

# [[mcp]]
# name = "git"
# command = "uvx"
# args = ["mcp-server-git"]

# [[mcp]]
# name = "github"
# command = "docker"
# args = ["run", "-i", "--rm", "-e", "GITHUB_PERSONAL_ACCESS_TOKEN", "ghcr.io/github/github-mcp-server"]
# env = { GITHUB_PERSONAL_ACCESS_TOKEN = "$GITHUB_PERSONAL_ACCESS_TOKEN" }
"#;
