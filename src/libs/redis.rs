use log::{debug, trace, warn};

use super::slack::{SlackUser, SlackUserGroup};
use crate::error::RedisErrors;
use std::collections::BTreeSet;
use std::time::Duration;

use derivative::Derivative;
use mobc::{Connection, Pool};
use mobc_redis::redis::{AsyncCommands, FromRedisValue};
use mobc_redis::{redis, RedisConnectionManager};

pub type MobcPool = Pool<RedisConnectionManager>;
pub type MobcCon = Connection<RedisConnectionManager>;
pub type Result<T> = std::result::Result<T, RedisErrors>;

const CACHE_POOL_MAX_OPEN: u64 = 16;
const CACHE_POOL_MAX_IDLE: u64 = 8;
const CACHE_POOL_TIMEOUT_SECONDS: u64 = 1;
const CACHE_POOL_EXPIRE_SECONDS: u64 = 60;
const REDIS_ENTITY_TIMEOUT_1_HOUR: usize = 60 * 60;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct RedisServer {
    #[derivative(Debug = "ignore")]
    redis_client: MobcPool,
}

#[derive(Debug, Eq, PartialEq, PartialOrd)]
enum RedisResult {
    String(String),
    Nil,
}

#[derive(Debug)]
pub enum RedisResponse<T, E> {
    Err(E),
    Missing,
    Ok(T),
}

impl RedisServer {
    pub async fn new(redis_address: &str) -> Result<Self> {
        let client: redis::Client = redis::Client::open(redis_address)
            .map_err(|e| RedisErrors::UnableToConnect(format!("{} - {}", e, redis_address)))?;
        let manager = RedisConnectionManager::new(client);
        let pool = Pool::builder()
            .get_timeout(Some(Duration::from_secs(CACHE_POOL_TIMEOUT_SECONDS)))
            .max_open(CACHE_POOL_MAX_OPEN)
            .max_idle(CACHE_POOL_MAX_IDLE)
            .max_lifetime(Some(Duration::from_secs(CACHE_POOL_EXPIRE_SECONDS)))
            .build(manager);

        Ok(Self { redis_client: pool })
    }

    pub async fn get_all_users(&self) -> RedisResponse<Vec<SlackUser>, RedisErrors> {
        let results: Result<Vec<SlackUser>> = self.str_scan("user:id:*").await;

        match results {
            Ok(value) => RedisResponse::Ok(value),
            Err(e) => RedisResponse::Err(e),
        }
    }

    pub async fn get_all_user_groups(&self) -> RedisResponse<Vec<SlackUserGroup>, RedisErrors> {
        let results: Result<Vec<SlackUserGroup>> = self.str_scan("user_group:id:*").await;

        match results {
            Ok(value) => RedisResponse::Ok(value),
            Err(e) => RedisResponse::Err(e),
        }
    }

    pub async fn get_user_by_id(&self, id: String) -> RedisResponse<SlackUser, RedisErrors> {
        self.unwrap_object(&format!("user:id:{}", id)).await
    }

    pub async fn get_user_by_email(&self, id: String) -> RedisResponse<SlackUser, RedisErrors> {
        self.unwrap_object(&format!("user:email:{}", id)).await
    }

    async fn unwrap_object<T>(&self, query_string: &str) -> RedisResponse<T, RedisErrors>
    where
        T: serde::de::DeserializeOwned + Clone,
    {
        match self.get_str(query_string).await {
            Err(e) => RedisResponse::Err(e),
            Ok(res) => match res {
                RedisResult::String(s) => match serde_json::from_str(&s) {
                    Ok(value) => RedisResponse::Ok(value),
                    Err(e) => RedisResponse::Err(RedisErrors::UnableToDeserialize(format!(
                        "Input: `{}`. Error: {}",
                        &s, e
                    ))),
                },
                RedisResult::Nil => RedisResponse::Missing,
            },
        }
    }

    pub async fn insert_users(&self, slack_users: &BTreeSet<SlackUser>) -> Result<()> {
        for user in slack_users {
            if let Err(e) = self
                .set_str(
                    &format!("user:email:{}", user.email),
                    &serde_json::to_string(&user).unwrap(),
                    REDIS_ENTITY_TIMEOUT_1_HOUR,
                )
                .await
            {
                warn!("Unable to insert {:?}. Error: {}", user, e);
            }

            if let Err(e) = self
                .set_str(
                    &format!("user:id:{}", user.id),
                    &serde_json::to_string(&user).unwrap(),
                    REDIS_ENTITY_TIMEOUT_1_HOUR,
                )
                .await
            {
                warn!("Unable to insert {:?}. Error: {}", user, e);
            }
        }

        Ok(())
    }

    pub async fn insert_user_groups(&self, slack_users: &BTreeSet<SlackUserGroup>) -> Result<()> {
        for group in slack_users {
            if let Err(e) = self
                .set_str(
                    &format!("user_group:id:{}", group.id),
                    &serde_json::to_string(&group).unwrap(),
                    REDIS_ENTITY_TIMEOUT_1_HOUR,
                )
                .await
            {
                warn!("Unable to insert {:?}. Error: {}", group, e);
            }

            if let Err(e) = self
                .set_str(
                    &format!("user_group:name:{}", group.name),
                    &serde_json::to_string(&group).unwrap(),
                    REDIS_ENTITY_TIMEOUT_1_HOUR,
                )
                .await
            {
                warn!("Unable to insert {:?}. Error: {}", group, e);
            }
        }

        Ok(())
    }

    pub async fn acquire_lock(&self, id: &str) -> Result<String> {
        let current_lock_holder = self.get_str("write_lock").await?;

        debug!("Current lock owner: {:?}", current_lock_holder);

        match current_lock_holder {
            RedisResult::String(s) => {
                if s != id {
                    return Ok(s);
                } else {
                    match self.set_str("write_lock", id, 120).await? {
                        RedisResult::Nil => Ok(id.to_string()),
                        RedisResult::String(s) => {
                            self.set_str("write_lock", &s, 120).await?;
                            Ok(s)
                        }
                    }
                }
            }
            RedisResult::Nil => match self.set_str("write_lock", id, 120).await? {
                RedisResult::Nil => Ok(id.to_string()),
                RedisResult::String(s) => {
                    self.set_str("write_lock", &s, 120).await?;
                    Ok(s)
                }
            },
        }
    }

    async fn set_str(&self, key: &str, value: &str, ttl_seconds: usize) -> Result<RedisResult> {
        let mut con = self.get_con().await?;
        let result = con
            .getset(key, value)
            .await
            .map_err(|e| RedisErrors::UnableToSet(format!("{}", e)))?;
        if ttl_seconds > 0 {
            con.expire(key, ttl_seconds)
                .await
                .map_err(|e| RedisErrors::UnableToExpire(format!("{}", e)))?;
        }
        trace!("SET `{:?}` => `{:?}` - RESULT: `{:?}`", key, value, result);

        if redis::Value::Nil == result {
            return Ok(RedisResult::Nil);
        }

        FromRedisValue::from_redis_value(&result)
            .map_err(|e| RedisErrors::UnableToReadValue(format!("{}", e)))
            .map(|s| RedisResult::String(s))
    }

    async fn str_scan<T>(&self, pattern: &str) -> Result<Vec<T>>
    where
        T: serde::de::DeserializeOwned,
    {
        let mut con = self.get_con().await?;
        let mut iter = con
            .scan_match(pattern)
            .await
            .map_err(|e| RedisErrors::UnableToGet(format!("{}", e)))?;

        trace!("SCAN `{}", pattern);

        let mut keys: BTreeSet<String> = BTreeSet::new();

        while let Some(element) = iter.next_item().await {
            if redis::Value::Nil == element {
                continue;
            }

            match String::from_redis_value(&element) {
                Err(e) => {
                    warn!("Unable to deserialize redis object: {}", e);
                    continue;
                }
                Ok(v) => {
                    keys.insert(v);
                }
            };
        }

        trace!("Number of elements to search over: {}", keys.len());

        if keys.is_empty() {
            return Ok(vec![]);
        }

        let mut results: Vec<_> = Vec::new();
        let values = con
            .get(keys)
            .await
            .map_err(|e| RedisErrors::UnableToGet(format!("{}", e)))?;

        let values = match values {
            redis::Value::Bulk(v) => v,
            _ => {
                warn!("Unable to fetch array");
                return Err(RedisErrors::UnableToGet(format!(
                    "Unable to fetch data from redis"
                )));
            }
        };

        for value in values {
            if redis::Value::Nil == value {
                continue;
            }

            let value = match String::from_redis_value(&value) {
                Err(e) => {
                    warn!("Unable to deserialize redis object: {}", e);
                    continue;
                }
                Ok(v) => v,
            };

            match serde_json::from_str::<T>(&value) {
                Ok(res) => {
                    results.push(res);
                }
                Err(e) => {
                    warn!("Unable to parse object. Input {}. Error: {}", &value, e);
                    continue;
                }
            }
        }

        Ok(results)
    }

    async fn get_str(&self, key: &str) -> Result<RedisResult> {
        let mut con = self.get_con().await?;
        let value = con
            .get(key)
            .await
            .map_err(|e| RedisErrors::UnableToGet(format!("{}", e)))?;

        trace!("GET `{:?}` - RESULT: `{:?}`", key, value);

        if redis::Value::Nil == value {
            return Ok(RedisResult::Nil);
        }

        FromRedisValue::from_redis_value(&value)
            .map_err(|e| RedisErrors::UnableToReadValue(format!("{}", e)))
            .map(|s| RedisResult::String(s))
    }

    async fn get_con(&self) -> Result<MobcCon> {
        self.redis_client
            .get()
            .await
            .map_err(|e| RedisErrors::UnableToConnect(format!("{}", e)))
    }
}
