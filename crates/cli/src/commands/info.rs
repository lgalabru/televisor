use clap::Args;
use televisor_core::Result;
use televisor_core::catalog::{Catalog, Resolution, ResolveOptions};

#[derive(Args)]
pub struct InfoCommand {
    /// Movie or series name (or numeric id)
    #[clap(required = true)]
    pub query: Vec<String>,
    /// Look up a series (show) instead of a movie
    #[clap(long)]
    pub series: bool,
    /// Include adult-flagged entries in name matching
    #[clap(long)]
    pub include_adult: bool,
    /// Print JSON instead of text
    #[clap(long)]
    pub json: bool,
    /// Account name (defaults to the configured default)
    #[clap(long)]
    pub account: Option<String>,
}

impl InfoCommand {
    pub async fn execute(&self) -> Result<()> {
        let (name, client) = super::client_for(self.account.as_deref())?;
        let catalog = Catalog::new(&client, &name)?;
        let query = self.query.join(" ");
        let opts = ResolveOptions {
            include_adult: self.include_adult,
            ..Default::default()
        };

        if self.series {
            let show = match catalog.resolve_series(&query, &opts).await? {
                Resolution::One(show) => show,
                Resolution::Ambiguous(candidates) => {
                    eprintln!("Ambiguous — {} series match '{query}':", candidates.len());
                    for c in &candidates {
                        eprintln!("{:>10}  {}", c.series_id, c.name);
                    }
                    std::process::exit(2);
                }
                Resolution::NotFound => {
                    eprintln!("No series matched '{query}'.");
                    std::process::exit(1);
                }
            };
            if self.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&show).expect("serialize")
                );
                return Ok(());
            }
            println!("{} (series id {})", show.name, show.series_id);
            print_field("Genre", show.genre.as_deref());
            print_field("Released", show.released());
            print_field("Rating", show.rating.map(|r| format!("{r}/10")).as_deref());
            print_field("Plot", show.plot.as_deref());
            return Ok(());
        }

        let movie = match catalog.resolve_movie(&query, &opts).await? {
            Resolution::One(movie) => movie,
            Resolution::Ambiguous(candidates) => {
                eprintln!("Ambiguous — {} movies match '{query}':", candidates.len());
                for c in &candidates {
                    eprintln!("{:>10}  {}", c.stream_id, c.name);
                }
                std::process::exit(2);
            }
            Resolution::NotFound => {
                eprintln!("No movie matched '{query}'. Try `televisor movies {query}`.");
                std::process::exit(1);
            }
        };
        let info = client.vod_info(movie.stream_id).await?;

        if self.json {
            let combined = serde_json::json!({ "movie": movie, "info": info });
            println!(
                "{}",
                serde_json::to_string_pretty(&combined).expect("serialize")
            );
            return Ok(());
        }
        println!("{} (id {})", movie.name, movie.stream_id);
        print_field("Genre", info.genre.as_deref());
        print_field("Released", info.released());
        print_field(
            "Rating",
            info.rating
                .or(movie.rating)
                .map(|r| format!("{r}/10"))
                .as_deref(),
        );
        print_field("Duration", info.duration.as_deref());
        print_field("Director", info.director.as_deref());
        print_field("Cast", info.cast.as_deref());
        print_field("Plot", info.plot.as_deref());
        if let Some(trailer) = &info.youtube_trailer {
            print_field(
                "Trailer",
                Some(format!("https://youtu.be/{trailer}")).as_deref(),
            );
        }
        Ok(())
    }
}

fn print_field(label: &str, value: Option<&str>) {
    if let Some(value) = value {
        println!("{label:<10}{value}");
    }
}
