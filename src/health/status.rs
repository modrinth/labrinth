use sqlx::{PgPool};
use actix_web::web;

pub async fn test_database(postgres: web::Data<PgPool>) -> Result<(), sqlx::Error> {
    let mut transaction = postgres.acquire().await?;
     sqlx::query(
        "
        SELECT 1
        "
    ).execute(&mut transaction)
         .await
         .map(|_| ())
}