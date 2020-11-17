use std::cmp::{Ord, Ordering};
use std::collections::BTreeSet;
use std::iter::FromIterator;

use log::{error, info, trace, warn};
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
        use slack_api::users::ListRequest;
        info!("Fetching all users from Slack");

        let all_users = match slack_api::users::list(
            &self.client,
            &self.token,
            &ListRequest { presence: None },
        )
        .await
        {
            Ok(results) => results,
            Err(e) => {
                error!("Unable to fetch data from Slack. Error: {}", e);
                return None;
            }
        };

        let all_users = match all_users.members {
            Some(users) => users,
            None => {
                warn!("Slack responded with no responses.");
                return None;
            }
        };

        let all_users: Vec<SlackUser> = all_users
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

        Some(BTreeSet::from_iter(all_users.into_iter()))
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
