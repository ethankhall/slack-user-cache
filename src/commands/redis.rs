use tracing::{debug, info, warn};

use crate::error::{CliErrors, SlackErrors};
use crate::UpdateRedisArgs;

use crate::libs::{RedisServer, SlackApi};

pub async fn redis_update(args: &UpdateRedisArgs) -> Result<(), CliErrors> {
    let redis_server = match RedisServer::new(&args.redis_address).await {
        Ok(redis_server) => redis_server,
        Err(e) => return Err(CliErrors::Redis(e)),
    };

    debug!("Getting server lock");
    let has_lock = redis_server.acquire_lock(&args.server_id).await?;
    if args.ignore_lock {
        warn!("Ignoring existing lock (if it exists). Be careful!");
    } else if has_lock {
        info!("Another server has the lock. Giving up");
        return Ok(());
    }
    debug!("Server lock acquired");

    let slack_api = SlackApi::new(&args.slack_token);

    debug!("Getting user profiles");
    let slack_users = match slack_api.list_all_users().await {
        None => return Err(CliErrors::Slack(SlackErrors::UnableToFetch)),
        Some(users) => users,
    };
    info!("Fetched {} users to save into redis", slack_users.len());

    debug!("Saving Users to Redis");
    redis_server.insert_users(&slack_users).await?;
    info!("{} users saved", slack_users.len());

    debug!("Getting user groups");
    let slack_user_groups = match slack_api.list_all_user_groups().await {
        None => return Err(CliErrors::Slack(SlackErrors::UnableToFetch)),
        Some(users) => users,
    };
    info!(
        "Fetched {} user groups to save into redis",
        slack_user_groups.len()
    );

    debug!("Saving User Groups to Redis");
    redis_server.insert_user_groups(&slack_user_groups).await?;
    info!("{} user groups saved", slack_user_groups.len());

    Ok(())
}
