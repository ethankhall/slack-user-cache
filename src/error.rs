use anyhow::Error as AnyhowError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CliErrors {
    #[error(transparent)]
    Redis(#[from] RedisErrors),

    #[error(transparent)]
    Slack(#[from] SlackErrors),
}

#[derive(Debug, Error)]
pub enum SlackErrors {
    #[error("Unable to fetch from Slack")]
    UnableToFetch,
}

#[derive(Debug, Error)]
pub enum RedisErrors {
    #[error("Unable to connect to {address}")]
    UnableToConnect {
        address: String,
        #[source]
        source: AnyhowError,
    },
    #[error("Unable to write {key} to redis")]
    UnableToSet {
        key: String,
        #[source]
        source: AnyhowError,
    },
    #[error("Unable to read {key} from redis")]
    UnableToGet {
        key: String,
        #[source]
        source: AnyhowError,
    },
    #[error("Unable to set {key} to expire")]
    UnableToExpire {
        key: String,
        #[source]
        source: AnyhowError,
    },
    #[error("Unable to read {key} value")]
    UnableToReadValue {
        key: String,
        #[source]
        source: AnyhowError,
    },
    #[error("Unable to deserialize {input}")]
    UnableToDeserialize {
        input: String,
        #[source]
        source: AnyhowError,
    },
}
