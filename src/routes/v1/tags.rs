use crate::database::models::categories::Category;
use crate::routes::ApiError;
use actix_web::HttpResponse;
use actix_web::{get, web};
use sqlx::PgPool;

#[get("category")]
pub async fn category_list(pool: web::Data<PgPool>) -> Result<HttpResponse, ApiError> {
    let results = Category::list(&**pool)
        .await?
        .into_iter()
        .filter(|x| x.project_type == "mod".to_string())
        .map(|x| x.project_type)
        .collect::<Vec<String>>();
    Ok(HttpResponse::Ok().json(results))
}
