use error_enum::{ErrorContainer, ErrorEnum, PrettyError};

#[derive(Debug, PartialEq, Eq, ErrorContainer)]
pub enum CliErrors {
    Redis(RedisErrors),
    Slack(SlackErrors),
}

#[derive(Debug, PartialEq, Eq, ErrorEnum)]
#[error_enum(prefix = "Slack")]
pub enum SlackErrors {
    #[error_enum(description = "Unable to fetch from Slack")]
    UnableToFetch,
}

#[derive(Debug, PartialEq, Eq, ErrorEnum)]
#[error_enum(prefix = "REDIS")]
pub enum RedisErrors {
    #[error_enum(description = "Unable to connect to Redis")]
    UnableToConnect(String),
    #[error_enum(description = "Unable to write to redis")]
    UnableToSet(String),
    #[error_enum(description = "Unable to read from redis")]
    UnableToGet(String),
    #[error_enum(description = "Unable to set expire")]
    UnableToExpire(String),
    #[error_enum(description = "Unable to read value")]
    UnableToReadValue(String),
    #[error_enum(description = "Unable to deserialize")]
    UnableToDeserialize(String),
}
