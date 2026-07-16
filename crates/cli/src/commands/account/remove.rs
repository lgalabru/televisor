use clap::Args;
use televisor_core::Result;
use televisor_core::config::ConfigFile;

#[derive(Args)]
pub struct RemoveCommand {
    /// Account name to remove
    pub name: String,
}

impl RemoveCommand {
    pub async fn execute(&self) -> Result<()> {
        let mut file = ConfigFile::open()?;
        file.remove_account(&self.name)?;
        file.save()?;
        println!("Removed account '{}'", self.name);
        Ok(())
    }
}
