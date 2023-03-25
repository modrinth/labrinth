use crate::database;
use crate::database::models::thread_item::ThreadMessageBuilder;
use crate::models::ids::ThreadMessageId;
use crate::models::threads::{
    MessageBody, Thread, ThreadId, ThreadMessage, ThreadType,
};
use crate::models::users::User;
use crate::routes::ApiError;
use crate::util::auth::get_user_from_headers;
use actix_web::{delete, get, patch, web, HttpRequest, HttpResponse};
use serde::Deserialize;
use sqlx::PgPool;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("thread")
            .service(thread_get)
            .service(thread_send_message),
    );
    cfg.service(web::scope("message").service(message_delete));
}

pub async fn is_authorized_thread(
    thread: &database::models::Thread,
    user: &User,
    pool: &PgPool,
) -> Result<bool, ApiError> {
    if user.role.is_mod() {
        return Ok(true);
    }

    let user_id: database::models::UserId = user.id.into();
    Ok(match thread.type_ {
        ThreadType::Report => {
            let report_exists = sqlx::query!(
                    "SELECT EXISTS(SELECT 1 FROM reports WHERE thread_id = $1 AND reporter = $2)",
                    thread.id as database::models::ids::ThreadId,
                    user_id as database::models::ids::UserId,
                )
                    .fetch_one(pool)
                    .await?
                    .exists;

            report_exists.unwrap_or(false)
        }
        ThreadType::Project => {
            let project_exists = sqlx::query!(
                "SELECT EXISTS(SELECT 1 FROM mods m INNER JOIN team_members tm ON tm.team_id = m.team_id AND tm.user_id = $2 WHERE thread_id = $1)",
                thread.id as database::models::ids::ThreadId,
                user_id as database::models::ids::UserId,
            )
                .fetch_one(pool)
                .await?
                .exists;

            project_exists.unwrap_or(false)
        }
        ThreadType::DirectMessage => thread.members.contains(&user_id),
    })
}

#[get("{id}")]
pub async fn thread_get(
    req: HttpRequest,
    info: web::Path<(ThreadId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let string = info.into_inner().0.into();

    let thread_data = database::models::Thread::get(string, &**pool).await?;

    let user = get_user_from_headers(req.headers(), &**pool).await?;

    if let Some(data) = thread_data {
        if is_authorized_thread(&data, &user, &pool).await? {
            let users: Vec<User> = database::models::User::get_many(
                &data
                    .messages
                    .iter()
                    .filter_map(|x| x.author_id)
                    .collect::<Vec<_>>(),
                &**pool,
            )
            .await?
            .into_iter()
            .map(From::from)
            .collect();

            let thread_type = data.type_;

            return Ok(HttpResponse::Ok().json(Thread {
                id: data.id.into(),
                type_: thread_type,
                messages: data
                    .messages
                    .into_iter()
                    .map(|x| ThreadMessage {
                        id: x.id.into(),
                        author_id: if thread_type == ThreadType::Report
                            && users
                                .iter()
                                .find(|y| x.author_id == Some(y.id.into()))
                                .map(|x| x.role.is_mod())
                                .unwrap_or(false)
                        {
                            None
                        } else {
                            x.author_id.map(|x| x.into())
                        },
                        body: x.body,
                        created: x.created,
                    })
                    .collect(),
                members: users,
            }));
        }
    }
    Ok(HttpResponse::NotFound().body(""))
}

#[derive(Deserialize)]
pub struct NewThreadMessage {
    pub body: MessageBody,
}

#[patch("{id}")]
pub async fn thread_send_message(
    req: HttpRequest,
    info: web::Path<(ThreadId,)>,
    pool: web::Data<PgPool>,
    new_message: web::Json<NewThreadMessage>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    if let MessageBody::Text { body } = &new_message.body {
        if body.len() > 65536 {
            return Err(ApiError::InvalidInput(
                "Input body is too long!".to_string(),
            ));
        }
    }

    let string: database::models::ThreadId = info.into_inner().0.into();
    let result = database::models::Thread::get(string, &**pool).await?;

    if let Some(thread) = result {
        if !is_authorized_thread(&thread, &user, &pool).await? {
            return Ok(HttpResponse::NotFound().body(""));
        }

        let mut transaction = pool.begin().await?;
        ThreadMessageBuilder {
            author_id: Some(user.id.into()),
            body: new_message.body.clone(),
            thread_id: thread.id,
        }
        .insert(&mut transaction)
        .await?;
        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}

#[delete("{id}")]
pub async fn message_delete(
    req: HttpRequest,
    info: web::Path<(ThreadMessageId,)>,
    pool: web::Data<PgPool>,
) -> Result<HttpResponse, ApiError> {
    let user = get_user_from_headers(req.headers(), &**pool).await?;

    let result = database::models::ThreadMessage::get(
        info.into_inner().0.into(),
        &**pool,
    )
    .await?;

    if let Some(thread) = result {
        if !user.role.is_mod() && thread.author_id != Some(user.id.into()) {
            return Err(ApiError::CustomAuthentication(
                "You cannot delete this message!".to_string(),
            ));
        }

        let mut transaction = pool.begin().await?;
        database::models::ThreadMessage::remove_full(
            thread.id,
            &mut transaction,
        )
        .await?;
        transaction.commit().await?;

        Ok(HttpResponse::NoContent().body(""))
    } else {
        Ok(HttpResponse::NotFound().body(""))
    }
}
