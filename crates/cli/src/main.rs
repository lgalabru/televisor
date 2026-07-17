use clap::{Parser, ValueEnum};
use tracing_subscriber::{EnvFilter, fmt};

mod commands;
mod no_dna;

#[derive(Parser)]
#[clap(
    name = "televisor",
    version,
    about = "Browse IPTV channels and open streams in VLC"
)]
struct Opts {
    /// Log output format
    #[clap(long, global = true, value_enum, default_value_t = LoggingFormat::Standard)]
    log_format: LoggingFormat,

    /// Machine-readable output for agents/automation (also via NO_DNA env)
    #[clap(long, global = true)]
    no_dna: bool,

    #[clap(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Interactive first-run: add an account and install the MCP server
    Setup(commands::setup::SetupCommand),
    /// Open a raw stream URL or file in VLC
    Open(commands::open::OpenCommand),
    /// Resolve a channel, movie (--movie), or episode (--series) and play it in VLC
    Play(commands::play::PlayCommand),
    /// List or search live channels
    Channels(commands::channels::ChannelsCommand),
    /// List or search movies (VOD)
    Movies(commands::movies::MoviesCommand),
    /// List or search series (shows)
    Series(commands::series::SeriesCommand),
    /// List a series' episodes
    Episodes(commands::episodes::EpisodesCommand),
    /// Show what's on a channel (EPG now/next or full schedule)
    Guide(commands::guide::GuideCommand),
    /// Show details for a movie or series (plot, rating, cast, …)
    Info(commands::info::InfoCommand),
    /// List live categories
    Categories(commands::categories::CategoriesCommand),
    /// Manage favorites (add/remove/list)
    #[clap(subcommand)]
    Favorites(commands::favorites::FavoritesCommand),
    /// Recently played items
    History(commands::history::HistoryCommand),
    /// Show account status (expiry, connections)
    Status(commands::status::StatusCommand),
    /// Manage panel accounts
    #[clap(subcommand)]
    Account(commands::account::AccountCommand),
    /// Run the MCP server on stdio (for AI agents)
    Mcp,
}

#[derive(Clone, Debug, ValueEnum)]
enum LoggingFormat {
    Standard,
    Json,
}

fn init_logging(format: &LoggingFormat) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    // Logs always go to stderr: stdout is command output — and the JSON-RPC
    // channel when running as an MCP server.
    match format {
        LoggingFormat::Standard => {
            fmt()
                .with_env_filter(filter)
                .with_target(true)
                .with_writer(std::io::stderr)
                .init();
        }
        LoggingFormat::Json => {
            fmt()
                .with_env_filter(filter)
                .json()
                .with_writer(std::io::stderr)
                .init();
        }
    }
}

#[tokio::main]
async fn main() {
    let opts = Opts::parse();
    if opts.no_dna {
        no_dna::enable_for_process();
    }
    // Agents parse stdout as JSON; force JSON logs (to stderr) so plain-text
    // log lines never confuse them.
    let log_format = if no_dna::is_agent() {
        LoggingFormat::Json
    } else {
        opts.log_format
    };
    init_logging(&log_format);

    let result: Result<(), String> = match opts.command {
        Command::Setup(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Open(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Play(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Channels(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Movies(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Series(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Episodes(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Guide(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Info(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Categories(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Favorites(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::History(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Status(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Account(cmd) => cmd.execute().await.map_err(|e| e.to_string()),
        Command::Mcp => televisor_mcp::run_server(&Default::default()).await,
    };

    if let Err(e) = result {
        if no_dna::is_agent() {
            no_dna::error_json(&e);
        } else {
            eprintln!("Error: {e}");
        }
        std::process::exit(1);
    }
}
