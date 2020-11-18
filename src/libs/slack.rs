use std::cmp::{Ord, Ordering};
use std::collections::BTreeSet;
use std::iter::FromIterator;

use log::{error, info, trace, debug, warn};
use serde::{Deserialize, Serialize};

use reqwest::Client;
use slack_api;
use slack_api::{User, Usergroup};

#[derive(Debug)]
pub struct SlackApi {
    client: Client,
    token: String,
}

#[serde(rename_all = "kebab-case")]
#[derive(Debug, Eq, PartialEq, PartialOrd, Serialize, Deserialize, Clone)]
pub struct SlackUserId {
    id: String,
}

impl Ord for SlackUserId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

#[serde(rename_all = "kebab-case")]
#[derive(Debug, Eq, PartialEq, PartialOrd, Serialize, Deserialize, Clone)]
pub struct SlackUser {
    pub id: String,
    pub name: String,
    pub email: String,
}

impl SlackUser {
    fn new(user: User) -> Result<Self, String> {
        let id: String = user.id.ok_or("no user id")?;
        let profile = user.profile.ok_or(format!("{}: no profile", id))?;

        let name: String = profile.real_name.ok_or(format!("{}: no name", id))?;
        let email: String = profile
            .email
            .ok_or(format!("{} - {}: no email", id, name))?;
        Ok(SlackUser { id, name, email })
    }
}

impl Ord for SlackUser {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

#[serde(rename_all = "kebab-case")]
#[derive(Debug, Eq, PartialEq, PartialOrd, Serialize, Deserialize, Clone)]
pub struct SlackUserGroup {
    pub name: String,
    pub id: String,
    pub users: BTreeSet<SlackUserId>,
}

impl Ord for SlackUserGroup {
    fn cmp(&self, other: &Self) -> Ordering {
        self.id.cmp(&other.id)
    }
}

impl SlackApi {
    pub fn new(token: &str) -> Self {
        Self {
            token: token.to_owned(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn list_all_users(&self) -> Option<BTreeSet<SlackUser>> {
        use governor::{Jitter, Quota, RateLimiter};
        use models::ListRequest;
        use nonzero_ext::*;
        use std::time::Duration;

        info!("Fetching all users from Slack");

        let mut cursor = None;
        let mut all_users = BTreeSet::new();
        let lim = RateLimiter::direct(Quota::per_minute(nonzero!(10u32)));
        let mut page_number = 0;

        loop {
            lim.until_ready_with_jitter(Jitter::up_to(Duration::from_secs(1)))
                .await;

            info!("Fetching page number {}", page_number);

            let paged_users = match models::list(
                &self.client,
                &self.token,
                &ListRequest {
                    limit: Some(200),
                    cursor: cursor,
                },
            )
            .await
            {
                Ok(results) => results,
                Err(e) => {
                    error!("Unable to fetch data from Slack. Error: {}", e);
                    return None;
                }
            };

            debug!("response_metadata: {:?}", paged_users.response_metadata);
            cursor = paged_users.response_metadata.next_cursor;

            let paged_users = match paged_users.members {
                Some(users) => users,
                None => {
                    warn!("Slack responded with no responses.");
                    return None;
                }
            };

            let paged_users: Vec<SlackUser> = paged_users
                .into_iter()
                .filter(|user| user.deleted == Some(false))
                .filter(|user| user.is_bot == Some(false))
                .map(|user| {
                    trace!("Raw User Data: {:?}", user);
                    SlackUser::new(user)
                })
                .filter_map(|res| {
                    if let Err(e) = res {
                        warn!("Unable to process user. Error: {}", e);
                        return None;
                    }
                    return Some(res);
                })
                .map(|user| user.unwrap())
                .collect();
            
            info!("Fetched {} users from page {}", paged_users.len(), page_number);
            
            all_users.extend(paged_users.into_iter());

            page_number = page_number + 1;

            if cursor == None || cursor == Some("".to_owned()) {
                break;
            }
        }

        Some(all_users)
    }

    pub async fn list_all_user_groups(&self) -> Option<BTreeSet<SlackUserGroup>> {
        use slack_api::usergroups::ListRequest;
        info!("Fetching all usergroups");

        let usergroup_list = match slack_api::usergroups::list(
            &self.client,
            &self.token,
            &ListRequest {
                include_disabled: Some(false),
                include_count: Some(false),
                include_users: Some(true),
            },
        )
        .await
        {
            Ok(results) => results,
            Err(e) => {
                error!("Unable to fetch data from Slack. Error: {}", e);
                return None;
            }
        };

        let usergroup_list = match usergroup_list.usergroups {
            Some(groups) => groups,
            None => {
                warn!("Slack responded with no responses.");
                return None;
            }
        };

        let mut result_slack_user_group: BTreeSet<SlackUserGroup> = BTreeSet::new();
        for usergroup in usergroup_list {
            if usergroup.deleted_by == None || usergroup.date_delete == None {
                continue;
            }
            let slack_user_group = self.build_user_group(usergroup).await;
            match slack_user_group {
                Ok(group) => {
                    result_slack_user_group.insert(group);
                }
                Err(e) => {
                    warn!("Unable to build usergroup: {}", e);
                }
            }
        }

        Some(result_slack_user_group)
    }

    async fn build_user_group(&self, user_group: Usergroup) -> Result<SlackUserGroup, String> {
        use slack_api::usergroups_users::ListRequest;
        let id = user_group.id.ok_or("no group id")?;
        let name = user_group.name.ok_or(format!("No name for group {}", id))?;

        let users = match slack_api::usergroups_users::list(
            &self.client,
            &self.token,
            &ListRequest {
                usergroup: &id,
                include_disabled: Some(false),
            },
        )
        .await
        {
            Ok(users) => users.users,
            Err(e) => {
                return Err(format!(
                    "Error getting users from group {}. Error: {}",
                    id, e
                ));
            }
        };

        let user_set = BTreeSet::from_iter(
            users
                .into_iter()
                .flatten()
                .map(|user_id| SlackUserId { id: user_id }),
        );

        Ok(SlackUserGroup {
            id: id.to_string(),
            name,
            users: user_set,
        })
    }
}

mod models {
    use serde::Deserialize;
    use slack_api::requests::SlackWebRequestSender;
    use slack_api::users::ListError;
    use slack_api::User;
    use std::error::Error;

    #[derive(Clone, Default, Debug)]
    pub struct ListRequest {
        /// Paginate through collections of data by setting
        pub cursor: Option<String>,
        /// Paginate through collections of data by setting
        pub limit: Option<u16>,
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct ResponseMetadata {
        pub next_cursor: Option<String>,
    }

    #[derive(Clone, Debug, Deserialize)]
    pub struct ListResponse {
        error: Option<String>,
        pub members: Option<Vec<User>>,
        #[serde(default)]
        ok: bool,
        pub response_metadata: ResponseMetadata,
    }

    impl<E: Error> Into<Result<ListResponse, ListError<E>>> for ListResponse {
        fn into(self) -> Result<ListResponse, ListError<E>> {
            if self.ok {
                Ok(self)
            } else {
                Err(self.error.as_ref().map(String::as_ref).unwrap_or("").into())
            }
        }
    }

    /// Lists all users in a Slack team.
    ///
    /// Wraps https://api.slack.com/methods/users.list

    pub async fn list<R>(
        client: &R,
        token: &str,
        request: &ListRequest,
    ) -> Result<ListResponse, ListError<R::Error>>
    where
        R: SlackWebRequestSender,
    {
        let params = vec![
            Some(("token", token.to_owned())),
            request
                .cursor
                .as_ref()
                .map(|cursor| ("cursor", cursor.clone())),
            request
                .limit
                .as_ref()
                .map(|limit| ("limit", limit.to_string())),
        ];
        let params = params.into_iter().filter_map(|x| x).collect::<Vec<_>>();
        let url = get_slack_url_for_method("users.list");
        client
            .send(&url, &params[..])
            .await
            .map_err(ListError::Client)
            .and_then(|result| {
                serde_json::from_str::<ListResponse>(&result)
                    .map_err(|e| ListError::MalformedResponse(result, e))
            })
            .and_then(|o| o.into())
    }

    fn get_slack_url_for_method(method: &str) -> String {
        format!("https://slack.com/api/{}", method)
    }
}
