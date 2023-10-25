use itertools::Itertools;

use super::{OrganizationId, UserId};
use crate::database::models::DatabaseError;

pub struct UserFollow {
    pub follower_id: UserId,
    pub target_id: UserId,
}

pub struct OrganizationFollow {
    pub follower_id: UserId,
    pub target_id: OrganizationId,
}

struct FollowQuery {
    follower_id: i64,
    target_id: i64,
}

impl From<FollowQuery> for UserFollow {
    fn from(value: FollowQuery) -> Self {
        UserFollow {
            follower_id: UserId(value.follower_id),
            target_id: UserId(value.target_id),
        }
    }
}

impl From<FollowQuery> for OrganizationFollow {
    fn from(value: FollowQuery) -> Self {
        OrganizationFollow {
            follower_id: UserId(value.follower_id),
            target_id: OrganizationId(value.target_id),
        }
    }
}

macro_rules! impl_follow {
    ($target_struct:ident, $table_name:tt, $target_id_type:ident, $target_id_ctor:expr) => {
        impl $target_struct {
            pub async fn insert(
                &self,
                exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
            ) -> Result<(), DatabaseError> {
                sqlx::query!(
                    " INSERT INTO " + $table_name + " (follower_id, target_id) VALUES ($1, $2)",
                    self.follower_id.0,
                    self.target_id.0
                )
                .execute(exec)
                .await?;

                Ok(())
            }

            pub async fn get_followers(
                target_id: $target_id_type,
                exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
            ) -> Result<Vec<$target_struct>, DatabaseError> {
                let res = sqlx::query_as!(
                    FollowQuery,
                    "SELECT follower_id, target_id FROM " + $table_name + " WHERE target_id=$1",
                    target_id.0
                )
                .fetch_all(exec)
                .await?;

                Ok(res.into_iter().map(|r| r.into()).collect_vec())
            }

            pub async fn get_follows_by_follower(
                follower_user_id: UserId,
                exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
            ) -> Result<Vec<$target_struct>, DatabaseError> {
                let res = sqlx::query_as!(
                    FollowQuery,
                    "SELECT follower_id, target_id FROM " + $table_name + " WHERE follower_id=$1",
                    follower_user_id.0
                )
                .fetch_all(exec)
                .await?;

                Ok(res.into_iter().map(|r| r.into()).collect_vec())
            }

            pub async fn unfollow(
                follower_id: UserId,
                target_id: $target_id_type,
                exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
            ) -> Result<(), DatabaseError> {
                sqlx::query!(
                    "DELETE FROM " + $table_name + " WHERE follower_id=$1 AND target_id=$2",
                    follower_id.0,
                    target_id.0,
                )
                .execute(exec)
                .await?;

                Ok(())
            }
        }
    };
}

impl_follow!(UserFollow, "user_follows", UserId, UserId);
impl_follow!(
    OrganizationFollow,
    "organization_follows",
    OrganizationId,
    OrganizationId
);
