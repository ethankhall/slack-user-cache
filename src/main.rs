use clap::{ArgGroup, Clap};
use dotenv::dotenv;
use tracing::error;

mod commands;
mod error;
mod libs;

#[derive(Clap, Debug)]
#[clap(group = ArgGroup::new("logging"))]
pub struct LoggingOpts {
    /// A level of verbosity, and can be used multiple times
    #[clap(short, long, parse(from_occurrences), global(true), group = "logging")]
    pub debug: u64,

    /// Enable warn logging
    #[clap(short, long, global(true), group = "logging")]
    pub warn: bool,

    /// Disable everything but error logging
    #[clap(short, long, global(true), group = "logging")]
    pub error: bool,
}

impl LoggingOpts {
    pub fn to_level(&self) -> tracing::Level {
        use tracing::Level;

        if self.error {
            Level::ERROR
        } else if self.warn {
            Level::WARN
        } else if self.debug == 0 {
            Level::INFO
        } else if self.debug == 1 {
            Level::DEBUG
        } else {
            Level::TRACE
        }
    }
}

#[derive(Clap, Debug)]
#[clap(author, about, version)]
struct Opts {
    #[clap(subcommand)]
    subcmd: SubCommand,
    #[clap(flatten)]
    logging_opts: LoggingOpts,
}

#[derive(Clap, Debug)]
enum SubCommand {
    /// When run, Slack will be queries and add it's results into Redis
    UpdateRedis(UpdateRedisArgs),
    /// Web server that serves results from `update-redis` sub-command
    Web(WebArgs),
}

#[derive(Clap, Debug)]
pub struct UpdateRedisArgs {
    /// Unique ID to identify the server
    #[clap(long, env = "SERVER_ID")]
    pub server_id: String,

    /// Slack API token. Permissions required: usergroups:read, users.profile:read, users:read, users:read.email
    #[clap(long, env = "SLACK_BOT_TOKEN")]
    pub slack_token: String,

    /// Address of the Redis Server
    #[clap(long, default_value = "redis://127.0.0.1/", env = "REDIS_ADDRESS")]
    pub redis_address: String,

    /// Disable everything but error logging
    #[clap(short, long)]
    pub ignore_lock: bool,
}

#[derive(Clap, Debug)]
pub struct WebArgs {
    /// Address of the Redis Server
    #[clap(long, default_value = "redis://127.0.0.1/", env = "REDIS_ADDRESS")]
    pub redis_address: String,

    /// Where the Server should listen on
    #[clap(long, default_value = "0.0.0.0:3000", env = "LISTEN_ADDRESS")]
    pub listen_server: String,
}

#[tokio::main]
pub async fn main() {
    dotenv().ok();

    let opt = Opts::parse();
    init_logger(&opt.logging_opts);
    let result = match opt.subcmd {
        SubCommand::UpdateRedis(args) => crate::commands::redis_update(&args).await,
        SubCommand::Web(args) => crate::commands::web_server(&args).await,
    };

    if let Err(e) = result {
        error!("Error: {}", e);
        std::process::exit(1);
    }
}

fn init_logger(logging_opts: &LoggingOpts) {
    use tracing_subscriber::FmtSubscriber;
    // a builder for `FmtSubscriber`.
    let subscriber = FmtSubscriber::builder()
        // all spans/events with a level higher than TRACE (e.g, debug, info, warn, etc.)
        // will be written to stdout.
        .with_max_level(logging_opts.to_level())
        // completes the builder.
        .finish();

    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");
}
