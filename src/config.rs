use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};

/// Top-level neutrino configuration
#[derive(Debug, Deserialize)]
pub struct Config {
    /// Agent to run in the VM
    pub agent: AgentConfig,
    /// VM provisioning settings
    pub vm: VmConfig,
    /// One-time setup commands run after first VM creation
    pub setup: Option<SetupConfig>,
    /// Command to run on the host after the VM is provisioned
    pub attach: Option<AttachConfig>,
    /// Secrets file to source environment variables from
    pub secrets: Option<SecretsConfig>,
    /// MCP server definitions
    #[serde(default, rename = "mcp")]
    pub mcp_servers: Vec<McpConfig>,
}

impl Config {
    pub fn from_file(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let base = config_base_dir(path).canonicalize()?;
        Self::parse(&content, &base)
    }

    pub fn parse(content: &str, base: &Path) -> anyhow::Result<Self> {
        let mut config: Self = toml::from_str(content).context("invalid config")?;
        if let Some(ref mut secrets) = config.secrets {
            secrets.source = resolve_path(base, &secrets.source);
        }
        Ok(config)
    }

    /// Returns all config fields available as template variables.
    ///
    /// Keys use dot-path notation matching the config structure (e.g. `config.vm.name`),
    /// so template strings like `{config.vm.name}` are substituted at runtime.
    pub fn template_vars(&self) -> HashMap<String, String> {
        macro_rules! vars {
            ($alias:ident = $config:expr; $($field:expr),+ $(,)?) => {{
                let $alias = $config;
                let mut map = HashMap::new();
                $(
                    map.insert(
                        stringify!($field).replace(" ", ""),
                        $field.to_string(),
                    );
                )+
                map
            }};
        }
        // Compile-time exhaustiveness check: update when VmConfig fields change.
        let _ = |v: &VmConfig| {
            let VmConfig {
                name: _,
                distro: _,
                memory_gb: _,
                cpus: _,
            } = v;
        };

        vars!(config = self;
            config.vm.name,
            config.vm.distro,
            config.vm.memory_gb,
            config.vm.cpus,
        )
    }
}

/// Agent configuration
#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    /// Agent type to use — currently only "claude" is supported
    #[serde(rename = "type")]
    pub agent_type: AgentType,
}

/// Supported agent types
#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentType {
    Claude,
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentType::Claude => write!(f, "claude"),
        }
    }
}

/// VM provisioning configuration
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct VmConfig {
    /// OrbStack VM name — must be unique per machine
    pub name: String,
    /// Distro image (e.g. "ubuntu:24.04")
    pub distro: String,
    /// Memory allocation in gigabytes
    pub memory_gb: u32,
    /// Number of virtual CPUs
    pub cpus: u32,
}

/// Secrets sourcing configuration
#[derive(Debug, Deserialize)]
pub struct SecretsConfig {
    /// Path to a file to source secrets from — canonicalized to absolute at parse time
    pub source: PathBuf,
}

/// One-time setup commands run inside the VM after first creation
#[derive(Debug, Deserialize)]
pub struct SetupConfig {
    /// Shell commands to run in sequence (each via `sh -c`)
    pub run: Vec<String>,
}

/// Host-side command to run after the VM is provisioned
///
/// Args may contain template variables (e.g. `{config.vm.name}`) which are
/// substituted with values from the config at runtime.
#[derive(Debug, Deserialize)]
pub struct AttachConfig {
    /// Executable to run on the host
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
}

impl AttachConfig {
    /// Return args with config template variables substituted.
    pub fn resolved_args(&self, config: &Config) -> Vec<String> {
        let vars = config.template_vars();
        self.args
            .iter()
            .map(|a| {
                vars.iter()
                    .fold(a.clone(), |s, (k, v)| s.replace(&format!("{{{k}}}"), v))
            })
            .collect()
    }
}

/// MCP server definition
#[derive(Debug, Deserialize)]
pub struct McpConfig {
    /// Server name
    pub name: String,
    /// Executable to run
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables — values prefixed with $ are resolved from the secrets file at provision time
    #[serde(default)]
    pub env: HashMap<String, String>,
}

fn config_base_dir(config_path: &Path) -> &Path {
    config_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn resolve_path(base: &Path, path: &Path) -> PathBuf {
    let lossy = path.to_string_lossy();
    let expanded = shellexpand::tilde(&lossy);
    let expanded = Path::new(expanded.as_ref());
    if expanded.is_absolute() {
        expanded.to_path_buf()
    } else {
        base.join(expanded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
        [agent]
        type = "claude"

        [vm]
        name = "test"
        distro = "ubuntu:24.04"
        memory_gb = 4
        cpus = 2
    "#;

    // resolve_path

    #[test]
    fn resolve_path_absolute_is_unchanged() {
        let base = Path::new("/some/base");
        let result = resolve_path(base, Path::new("/absolute/path/.env"));
        assert_eq!(result, Path::new("/absolute/path/.env"));
    }

    #[test]
    fn resolve_path_relative_joins_base() {
        let base = Path::new("/some/base");
        let result = resolve_path(base, Path::new(".env"));
        assert_eq!(result, Path::new("/some/base/.env"));
    }

    #[test]
    fn resolve_path_tilde_expands_to_home() {
        let base = Path::new("/some/base");
        let result = resolve_path(base, Path::new("~/.env"));
        let home = PathBuf::from(std::env::var("HOME").unwrap());
        assert_eq!(result, home.join(".env"));
    }

    // Config::parse

    #[test]
    fn parse_minimal_config() {
        Config::parse(MINIMAL, Path::new("/project")).unwrap();
    }

    #[test]
    fn parse_resolves_secrets_path() {
        let base = Path::new("/project");
        let content = format!("{MINIMAL}\n[secrets]\nsource = \".env\"");
        let config = Config::parse(&content, base).unwrap();
        assert_eq!(config.secrets.unwrap().source, Path::new("/project/.env"));
    }

    #[test]
    fn parse_invalid_toml_returns_error() {
        let base = Path::new("/project");
        assert!(Config::parse("this is not toml ][", base).is_err());
    }

    #[test]
    fn parse_missing_required_field_returns_error() {
        let base = Path::new("/project");
        // vm.cpus is missing
        let content = r#"
            [agent]
            type = "claude"
            [vm]
            name = "test"
            distro = "ubuntu:24.04"
            memory_gb = 4
        "#;
        assert!(Config::parse(content, base).is_err());
    }

    // AttachConfig::resolved_args

    #[test]
    fn resolved_args_substitutes_vm_name() {
        let config = Config::parse(MINIMAL, Path::new("/project")).unwrap();
        let attach = AttachConfig {
            command: "orb".into(),
            args: vec!["run".into(), "-m".into(), "{config.vm.name}".into()],
        };
        assert_eq!(attach.resolved_args(&config), vec!["run", "-m", "test"]);
    }

    #[test]
    fn resolved_args_passes_through_non_template_args() {
        let config = Config::parse(MINIMAL, Path::new("/project")).unwrap();
        let attach = AttachConfig {
            command: "zed".into(),
            args: vec!["ssh://orb/home/ubuntu".into()],
        };
        assert_eq!(attach.resolved_args(&config), vec!["ssh://orb/home/ubuntu"]);
    }
}
