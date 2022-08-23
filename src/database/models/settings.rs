use crate::database::models::UserId;
use crate::models::settings::FrontendTheme;
use serde::Serialize;

#[derive(Serialize)]
pub struct UserSettings {
    pub tos_agreed: bool,
    pub public_email: bool,
    pub public_github: bool,
    pub theme: FrontendTheme,
}

impl UserSettings {
    pub async fn tos_agreed<'a, E>(
        user_id: UserId,
        exec: E,
    ) -> Result<bool, sqlx::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        let result = sqlx::query!(
            "
            SELECT tos_agreed
            FROM user_settings
            WHERE user_id = $1
            ",
            user_id as crate::database::models::ids::UserId
        )
        .fetch_one(exec)
        .await?;

        Ok(result.tos_agreed)
    }

    pub async fn public_email<'a, E>(
        user_id: UserId,
        exec: E,
    ) -> Result<bool, sqlx::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT public_email
            FROM user_settings
            WHERE user_id = $1
            ",
            user_id as crate::database::models::ids::UserId
        )
        .fetch_one(exec)
        .await?;

        Ok(result.public_email)
    }

    pub async fn public_github<'a, E>(
        user_id: UserId,
        exec: E,
    ) -> Result<bool, sqlx::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT public_github
            FROM user_settings
            WHERE user_id = $1
            ",
            user_id as crate::database::models::ids::UserId
        )
        .fetch_one(exec)
        .await?;

        Ok(result.public_github)
    }

    pub async fn theme<'a, E>(
        user_id: UserId,
        exec: E,
    ) -> Result<FrontendTheme, sqlx::Error>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres> + Copy,
    {
        let result = sqlx::query!(
            "
            SELECT theme
            FROM user_settings
            WHERE user_id = $1
            ",
            user_id as crate::database::models::ids::UserId
        )
        .fetch_one(exec)
        .await?;

        Ok(FrontendTheme::from_str(&result.theme))
    }
}
