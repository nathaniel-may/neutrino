use std::collections::HashMap;

use crate::agent::parse_env_file;
use crate::config::Config;
use crate::vm;

pub fn run_if_needed(config: &Config) -> anyhow::Result<()> {
    let setup = match &config.setup {
        Some(s) => s,
        None => return Ok(()),
    };

    let already_done = vm::capture(
        &config.vm.name,
        &[
            "sh",
            "-c",
            "test -f ~/.neutrino-setup-done && echo yes || true",
        ],
    )?;
    if already_done == "yes" {
        return Ok(());
    }

    let secrets = match &config.secrets {
        Some(s) => {
            let content = std::fs::read_to_string(&s.source)?;
            parse_env_file(&content)
        }
        None => HashMap::new(),
    };

    println!("Running setup...");
    for cmd in &setup.run {
        let expanded = expand_secrets(cmd, &secrets);
        vm::run(&config.vm.name, &["sh", "-c", &expanded])?;
    }
    vm::run(&config.vm.name, &["touch", ".neutrino-setup-done"])?;
    println!("Setup done.");
    Ok(())
}

/// Substitute $VAR references in a string using values from the secrets map.
/// Unrecognised variables are left as-is.
fn expand_secrets(s: &str, secrets: &HashMap<String, String>) -> String {
    let mut result = s.to_string();
    for (k, v) in secrets {
        result = result.replace(&format!("${k}"), v);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_secrets_substitutes_known_vars() {
        let secrets = HashMap::from([("TOKEN".into(), "abc123".into())]);
        assert_eq!(
            expand_secrets("https://oauth2:$TOKEN@github.com/repo.git", &secrets),
            "https://oauth2:abc123@github.com/repo.git"
        );
    }

    #[test]
    fn expand_secrets_leaves_unknown_vars() {
        let secrets = HashMap::new();
        assert_eq!(expand_secrets("echo $UNKNOWN", &secrets), "echo $UNKNOWN");
    }

    #[test]
    fn expand_secrets_no_vars() {
        let secrets = HashMap::from([("TOKEN".into(), "abc123".into())]);
        assert_eq!(expand_secrets("git status", &secrets), "git status");
    }
}
