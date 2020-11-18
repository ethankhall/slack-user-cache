use std::sync::Arc;

use serde_json::json;
use warp::http::StatusCode;
use warp::Filter;

use log::{debug, info};

type Db = Arc<RedisServer>;

use crate::error::CliErrors;
use crate::libs::RedisServer;
use crate::{init_logger, LoggingOpts, WebArgs};

enum Response<T>
where
    T: serde::Serialize,
{
    Result { result: T },
    Error { message: String },
    NotFound,
}

impl<T> Response<T>
where
    T: serde::Serialize,
{
    fn into_response(self) -> warp::reply::WithStatus<warp::reply::Json> {
        match self {
            Response::Result { result } => {
                let obj = json!({
                    "code": 200,
                    "success": true,
                    "result": result
                });

                warp::reply::with_status(warp::reply::json(&obj), StatusCode::OK)
            }
            Response::Error { message } => {
                let obj = json!({
                    "code": 501,
                    "success": false,
                    "message": message
                });

                warp::reply::with_status(warp::reply::json(&obj), StatusCode::INTERNAL_SERVER_ERROR)
            }
            Response::NotFound => {
                let obj = json!({
                    "code": 404,
                    "success": true,
                    "message": "not found"
                });

                warp::reply::with_status(warp::reply::json(&obj), StatusCode::NOT_FOUND)
            }
        }
    }
}

pub async fn web_server(root_logger: &LoggingOpts, args: &WebArgs) -> Result<(), CliErrors> {
    use std::net::SocketAddr;

    init_logger(&LoggingOpts::merge(&root_logger, &args.logging_opts));

    let redis_server = match RedisServer::new(&args.redis_address).await {
        Ok(redis_server) => redis_server,
        Err(e) => return Err(CliErrors::Redis(e)),
    };

    debug!("Redis client create");

    let db = Arc::new(redis_server);

    let api = filters::get_all_users(db.clone())
        .or(filters::get_user_by_id(db.clone()))
        .or(filters::get_user_by_email(db.clone()))
        .or(filters::get_all_user_groups(db.clone()))
        .or(filters::status());

    let listen_server: SocketAddr = args
        .listen_server
        .parse()
        .expect("Unable to parse listen_server");

    info!("Listing on {}", listen_server);

    warp::serve(api).run(listen_server).await;

    Ok(())
}

mod filters {
    use super::{handlers, Db};
    use std::convert::Infallible;
    use warp::Filter;

    pub fn get_all_users(
        db: Db,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("slack" / "users")
            .and(warp::get())
            .and(with_db(db))
            .and_then(handlers::get_all_users)
    }

    pub fn get_user_by_id(
        db: Db,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("slack" / "user" / "id" / String)
            .and(warp::get())
            .and(with_db(db))
            .and_then(handlers::get_user_by_id)
    }

    pub fn get_user_by_email(
        db: Db,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("slack" / "user" / "email" / String)
            .and(warp::get())
            .and(with_db(db))
            .and_then(handlers::get_user_by_email)
    }

    pub fn get_all_user_groups(
        db: Db,
    ) -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("slack" / "user_groups")
            .and(warp::get())
            .and(with_db(db))
            .and_then(handlers::get_all_user_groups)
    }

    pub fn status() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
        warp::path!("healthz").map(|| {
            super::Response::Result {
                result: "OK".to_owned(),
            }
            .into_response()
        })
    }

    fn with_db(db: Db) -> impl Filter<Extract = (Db,), Error = Infallible> + Clone {
        warp::any().map(move || db.clone())
    }
}

mod handlers {
    use super::{Db, Response};
    use crate::libs::RedisResponse;
    use std::convert::Infallible;

    pub async fn get_all_user_groups(redis_server: Db) -> Result<impl warp::Reply, Infallible> {
        let result = match redis_server.get_all_user_groups().await {
            RedisResponse::Ok(results) => Response::Result { result: results },
            RedisResponse::Err(e) => Response::Error {
                message: format!("{}", e),
            },
            RedisResponse::Missing => Response::NotFound,
        };

        Ok(result.into_response())
    }

    pub async fn get_all_users(redis_server: Db) -> Result<impl warp::Reply, Infallible> {
        let result = match redis_server.get_all_users().await {
            RedisResponse::Ok(results) => Response::Result { result: results },
            RedisResponse::Err(e) => Response::Error {
                message: format!("{}", e),
            },
            RedisResponse::Missing => Response::NotFound,
        };

        Ok(result.into_response())
    }

    pub async fn get_user_by_id(
        id: String,
        redis_server: Db,
    ) -> Result<impl warp::Reply, Infallible> {
        let result = match redis_server.get_user_by_id(id).await {
            RedisResponse::Ok(results) => Response::Result { result: results },
            RedisResponse::Err(e) => Response::Error {
                message: format!("{}", e),
            },
            RedisResponse::Missing => Response::NotFound,
        };

        Ok(result.into_response())
    }

    pub async fn get_user_by_email(
        email: String,
        redis_server: Db,
    ) -> Result<impl warp::Reply, Infallible> {
        let result = match redis_server.get_user_by_email(email).await {
            RedisResponse::Ok(results) => Response::Result { result: results },
            RedisResponse::Err(e) => Response::Error {
                message: format!("{}", e),
            },
            RedisResponse::Missing => Response::NotFound,
        };

        Ok(result.into_response())
    }
}
