use clap::Clap;
use dotenv::dotenv;
use log::error;

mod commands;
mod error;
mod libs;

#[derive(Clap, Debug)]
pub struct LoggingOpts {
    /// A level of verbosity, and can be used multiple times
    #[clap(short, long, parse(from_occurrences), group = "logging")]
    verbose: u64,

    /// Enable all logging
    #[clap(short, long, group = "logging")]
    debug: bool,

    /// Disable everything but error logging
    #[clap(short, long, group = "logging")]
    error: bool,
}

impl LoggingOpts {
    pub fn merge(right: &LoggingOpts, left: &LoggingOpts) -> LoggingOpts {
        if right.debug || left.debug {
            LoggingOpts {
                debug: true,
                error: false,
                verbose: 0,
            }
        } else if right.verbose != 0 || left.verbose != 0 {
            LoggingOpts {
                verbose: std::cmp::max(right.verbose, left.verbose),
                debug: false,
                error: false,
            }
        } else {
            LoggingOpts {
                debug: true,
                error: right.error || left.error,
                verbose: 0,
            }
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
    #[clap(flatten)]
    pub logging_opts: LoggingOpts,

    /// Unique ID to identify the server
    #[clap(long, env = "SERVER_ID")]
    pub server_id: String,
    
    /// Slack API token. Permissions required: usergroups:read, users.profile:read, users:read
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
    #[clap(flatten)]
    pub logging_opts: LoggingOpts,
    
    /// Address of the Redis Server
    #[clap(long, default_value = "redis://127.0.0.1/", env = "REDIS_ADDRESS")]
    pub redis_address: String,
    
    /// Where the Server should listen on
    #[clap(long, default_value = "0.0.0.0:3000", env = "LISTEN_ADDRESS")]
    pub listen_server: String
}

#[tokio::main]
pub async fn main() {
    dotenv().ok();

    let opt = Opts::parse();
    let result = match opt.subcmd {
        SubCommand::UpdateRedis(args) => {
            crate::commands::redis_update(&opt.logging_opts, &args).await
        }
        SubCommand::Web(args) => crate::commands::web_server(&opt.logging_opts, &args).await,
    };

    if let Err(e) = result {
        error!("Error: {}", e);
        std::process::exit(e.get_error_number().into());
    }
}

pub(crate) fn init_logger(logging_opts: &LoggingOpts) {
    let mut logger = loggerv::Logger::new();
    if logging_opts.debug {
        logger = logger
            .verbosity(10)
            .line_numbers(true)
            .add_module_path_filter(module_path!());
    } else if logging_opts.error {
        logger = logger.verbosity(0).add_module_path_filter(module_path!());
    } else {
        logger = logger
            .base_level(log::Level::Info)
            .verbosity(logging_opts.verbose)
            .line_numbers(true)
            .add_module_path_filter(module_path!());
    }

    logger.init().unwrap();
}
