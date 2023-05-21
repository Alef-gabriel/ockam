use clap::Args;
use miette::miette;
use ockam_api::cli_state::traits::StateDirTrait;

use crate::CommandGlobalOpts;

/// List vaults
#[derive(Clone, Debug, Args)]
pub struct ListCommand;

impl ListCommand {
    pub fn run(self, opts: CommandGlobalOpts) {
        if let Err(e) = run_impl(opts) {
            eprintln!("{e}");
            std::process::exit(e.code());
        }
    }
}

fn run_impl(opts: CommandGlobalOpts) -> crate::Result<()> {
    let states = opts.state.vaults.list()?;
    if states.is_empty() {
        return Err(miette!("No vaults registered on this system!").into());
    }
    for (idx, vault) in states.iter().enumerate() {
        println!("Vault[{idx}]:");
        for line in vault.to_string().lines() {
            println!("{:2}{}", "", line)
        }
    }
    Ok(())
}
