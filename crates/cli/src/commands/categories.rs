use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::Catalog;

#[derive(Args)]
pub struct CategoriesCommand {
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
    /// Refetch from the panel instead of the local cache
    #[clap(long)]
    pub refresh: bool,
    /// Print JSON instead of a table
    #[clap(long)]
    pub json: bool,
}

impl CategoriesCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, source) = super::client_for(self.account.as_deref())?;
        let catalog = Catalog::new(&source, &name)?;
        let categories = catalog.categories(self.refresh).await?;

        if crate::no_dna::should_json(self.json) {
            crate::no_dna::emit_json(&categories);
        } else {
            for category in &categories {
                println!("{:>8}  {}", category.category_id, category.category_name);
            }
        }
        Ok(())
    }
}
