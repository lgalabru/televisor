use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};
use televisor_core::state::{Favorite, MediaKind, StateFile};

#[derive(clap::Subcommand)]
pub enum FavoritesCommand {
    /// Add a channel (default), movie, or series episode-source to favorites
    Add(AddCommand),
    /// Remove a favorite by name or id
    Remove(RemoveCommand),
    /// List favorites
    List(ListCommand),
}

impl FavoritesCommand {
    pub async fn execute(&self) -> Result<()> {
        match self {
            FavoritesCommand::Add(cmd) => cmd.execute().await,
            FavoritesCommand::Remove(cmd) => cmd.execute().await,
            FavoritesCommand::List(cmd) => cmd.execute().await,
        }
    }
}

#[derive(Args)]
pub struct AddCommand {
    /// Channel/movie name or numeric id
    #[clap(required = true)]
    pub query: Vec<String>,
    /// The favorite is a movie
    #[clap(long)]
    pub movie: bool,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl AddCommand {
    pub async fn execute(&self) -> Result<()> {
        let (account, kind, id, name) =
            resolve_target(&self.query.join(" "), self.movie, self.account.as_deref()).await?;
        let mut file = StateFile::open()?;
        let added = file.add_favorite(
            &account,
            Favorite {
                kind,
                id,
                name: name.clone(),
            },
        );
        file.save()?;
        if added {
            println!("★ {name} (id {id}) added to favorites");
        } else {
            println!("{name} is already a favorite");
        }
        Ok(())
    }
}

#[derive(Args)]
pub struct RemoveCommand {
    /// Channel/movie name or numeric id
    #[clap(required = true)]
    pub query: Vec<String>,
    /// The favorite is a movie
    #[clap(long)]
    pub movie: bool,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl RemoveCommand {
    pub async fn execute(&self) -> Result<()> {
        let (account, kind, id, name) =
            resolve_target(&self.query.join(" "), self.movie, self.account.as_deref()).await?;
        let mut file = StateFile::open()?;
        let removed = file.remove_favorite(&account, kind, id);
        file.save()?;
        if removed {
            println!("Removed {name} from favorites");
        } else {
            println!("{name} wasn't a favorite");
        }
        Ok(())
    }
}

#[derive(Args)]
pub struct ListCommand {
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl ListCommand {
    pub async fn execute(&self) -> Result<()> {
        let (account, _source) = super::client_for(self.account.as_deref())?;
        let file = StateFile::open()?;
        let favorites = file.favorites(&account);
        if crate::no_dna::is_agent() {
            crate::no_dna::emit_json(&favorites);
            return Ok(());
        }
        if favorites.is_empty() {
            println!("No favorites yet — `televisor favorites add <channel>`.");
            return Ok(());
        }
        for favorite in favorites {
            println!(
                "{:>10}  [{}]  {}",
                favorite.id, favorite.kind, favorite.name
            );
        }
        Ok(())
    }
}

/// Resolve the query against the right catalog and return what to store.
async fn resolve_target(
    query: &str,
    movie: bool,
    account: Option<&str>,
) -> Result<(String, MediaKind, u64, String)> {
    let (name, source) = super::client_for(account)?;
    let catalog = Catalog::new(&source, &name)?;
    let opts = ResolveOptions::default();
    if movie {
        match catalog.resolve_movie(query, &opts).await? {
            Resolution::One(m) => Ok((name, MediaKind::Movie, m.stream_id.0, m.name)),
            Resolution::Ambiguous(candidates) => {
                eprintln!("Ambiguous — {} movies match '{query}':", candidates.len());
                for c in &candidates {
                    eprintln!("{:>10}  {}", c.stream_id, c.name);
                }
                std::process::exit(2);
            }
            Resolution::NotFound => {
                eprintln!("No movie matched '{query}'.");
                std::process::exit(1);
            }
        }
    } else {
        match catalog.resolve(query, &opts).await? {
            Resolution::One(s) => Ok((name, MediaKind::Live, s.stream_id.0, s.name)),
            Resolution::Ambiguous(candidates) => {
                eprintln!("Ambiguous — {} channels match '{query}':", candidates.len());
                for c in &candidates {
                    eprintln!("{:>10}  {}", c.stream_id, c.name);
                }
                std::process::exit(2);
            }
            Resolution::NotFound => {
                eprintln!("No channel matched '{query}'.");
                std::process::exit(1);
            }
        }
    }
}
