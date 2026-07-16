use clap::Args;
use televisor_core::Result;
use televisor_core::config::ConfigFile;

#[derive(Args)]
pub struct SetDefaultCommand {
    /// Account name to make the default
    pub name: String,
}

impl SetDefaultCommand {
    pub async fn execute(&self) -> Result<()> {
        let mut file = ConfigFile::open()?;
        file.set_default(&self.name)?;
        file.save()?;
        println!("Default account is now '{}'", self.name);
        Ok(())
    }
}
