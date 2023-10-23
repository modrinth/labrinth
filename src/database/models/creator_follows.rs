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

impl UserFollow {
    pub async fn insert(
        &self,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            INSERT INTO user_follows (follower_id, target_id) VALUES ($1, $2)
            ",
            self.follower_id.0,
            self.target_id.0
        )
        .execute(exec)
        .await?;

        Ok(())
    }

    pub async fn get_followers(
        target_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<UserFollow>, DatabaseError> {
        let res = sqlx::query!(
            "
            SELECT follower_id, target_id FROM user_follows
            WHERE target_id=$1
            ",
            target_id.0
        )
        .fetch_all(exec)
        .await?;

        Ok(res
            .into_iter()
            .map(|r| UserFollow {
                follower_id: UserId(r.follower_id),
                target_id: UserId(r.target_id),
            })
            .collect_vec())
    }

    pub async fn get_follows_from(
        user_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<UserFollow>, DatabaseError> {
        let res = sqlx::query!(
            "
            SELECT follower_id, target_id FROM user_follows
            WHERE follower_id=$1
            ",
            user_id.0
        )
        .fetch_all(exec)
        .await?;

        Ok(res
            .into_iter()
            .map(|r| UserFollow {
                follower_id: UserId(r.follower_id),
                target_id: UserId(r.target_id),
            })
            .collect_vec())
    }

    pub async fn unfollow(
        follower_id: UserId,
        target_id: UserId,
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        sqlx::query!(
            "
            DELETE FROM user_follows
            WHERE follower_id=$1 AND target_id=$2
            ",
            follower_id.0,
            target_id.0,
        )
        .execute(exec)
        .await?;

        Ok(())
    }
}
