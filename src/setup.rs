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

    println!("Running setup...");
    for cmd in &setup.run {
        vm::run(&config.vm.name, &["sh", "-c", cmd])?;
    }
    vm::run(&config.vm.name, &["touch", ".neutrino-setup-done"])?;
    println!("Setup done.");
    Ok(())
}
