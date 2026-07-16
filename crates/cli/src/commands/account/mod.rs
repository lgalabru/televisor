pub mod add;
pub mod list;
pub mod remove;
pub mod set_default;

use televisor_core::Result;

#[derive(clap::Subcommand)]
pub enum AccountCommand {
    /// Add a panel account
    Add(add::AddCommand),
    /// List configured accounts (never prints passwords)
    List(list::ListCommand),
    /// Remove an account
    Remove(remove::RemoveCommand),
    /// Set the default account
    SetDefault(set_default::SetDefaultCommand),
}

impl AccountCommand {
    pub async fn execute(&self) -> Result<()> {
        match self {
            AccountCommand::Add(cmd) => cmd.execute().await,
            AccountCommand::List(cmd) => cmd.execute().await,
            AccountCommand::Remove(cmd) => cmd.execute().await,
            AccountCommand::SetDefault(cmd) => cmd.execute().await,
        }
    }
}
