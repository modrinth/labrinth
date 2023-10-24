use std::convert::{TryFrom, TryInto};

use super::{
    dynamic::{DynamicId, IdType},
    generate_event_id, DatabaseError, EventId, OrganizationId, ProjectId, UserId,
};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::postgres::{PgHasArrayType, PgTypeInfo};

#[derive(sqlx::Type)]
#[sqlx(type_name = "event_type", rename_all = "snake_case")]
pub enum EventType {
    ProjectCreated,
}

pub enum CreatorId {
    User(UserId),
    Organization(OrganizationId),
}

pub enum EventData {
    ProjectCreated {
        project_id: ProjectId,
        creator_id: CreatorId,
    },
}

pub struct Event {
    id: EventId,
    event_data: EventData,
}

impl Event {
    pub async fn new(
        event_data: EventData,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, DatabaseError> {
        let id = generate_event_id(transaction).await?;
        Ok(Self { id, event_data })
    }
}

impl From<CreatorId> for DynamicId {
    fn from(value: CreatorId) -> Self {
        match value {
            CreatorId::User(user_id) => user_id.into(),
            CreatorId::Organization(organization_id) => organization_id.into(),
        }
    }
}

impl TryFrom<DynamicId> for CreatorId {
    type Error = DatabaseError;

    fn try_from(value: DynamicId) -> Result<Self, Self::Error> {
        match value.id_type {
            IdType::UserId => Ok(CreatorId::User(value.try_into()?)),
            _ => Ok(CreatorId::Organization(value.try_into()?)),
        }
    }
}

impl From<Event> for RawEvent {
    fn from(value: Event) -> Self {
        match value.event_data {
            EventData::ProjectCreated {
                project_id,
                creator_id,
            } => RawEvent {
                id: value.id,
                target_id: project_id.into(),
                triggerer_id: Some(creator_id.into()),
                event_type: EventType::ProjectCreated,
                metadata: None,
                created: None,
            },
        }
    }
}

impl TryFrom<RawEvent> for Event {
    type Error = DatabaseError;

    fn try_from(value: RawEvent) -> Result<Self, Self::Error> {
        Ok(match value.event_type {
            EventType::ProjectCreated => Event {
                id: value.id,
                event_data: EventData::ProjectCreated {
                    project_id: value.target_id.try_into()?,
                    creator_id: value.triggerer_id.map_or_else(
                        || {
                            Err(DatabaseError::UnexpectedNull(
                                "triggerer_id should not be null for project creation".to_string(),
                            ))
                        },
                        |v| v.try_into(),
                    )?,
                },
            },
        })
    }
}

struct RawEvent {
    pub id: EventId,
    pub target_id: DynamicId,
    pub triggerer_id: Option<DynamicId>,
    pub event_type: EventType,
    // #[serde::serde(flatten)] //TODO: is this necessary?
    pub metadata: Option<serde_json::Value>,
    pub created: Option<DateTime<Utc>>,
}

impl PgHasArrayType for EventType {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("event_type")
    }
}

impl PgHasArrayType for DynamicId {
    fn array_type_info() -> PgTypeInfo {
        PgTypeInfo::with_name("dynamic_id")
    }
}

impl Event {
    pub async fn insert(
        self,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        Self::insert_many(vec![self], transaction).await
    }

    pub async fn insert_many(
        events: Vec<Self>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        RawEvent::insert_many(
            events.into_iter().map(|e| e.into()).collect_vec(),
            transaction,
        )
        .await
    }

    pub async fn get_triggerer_feed(
        triggerer_ids: &[DynamicId],
        event_types: &[EventType],
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<Event>, DatabaseError> {
        Ok(sqlx::query_as!(
            RawEvent,
            r#"
            SELECT 
                id,
                target_id as "target_id: _",
                triggerer_id as "triggerer_id: _",
                type as "event_type: _",
                metadata,
                created
            FROM events e
            WHERE triggerer_id=ANY($1) AND type=ANY($2)
            "#,
            &triggerer_ids[..] as &[DynamicId],
            &event_types[..] as &[EventType]
        )
        .fetch_all(exec)
        .await?
        .into_iter()
        .map(|r| r.try_into())
        .collect::<Result<Vec<_>, _>>()?)
    }
}

impl RawEvent {
    pub async fn insert_many(
        events: Vec<Self>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
        let (ids, target_ids, triggerer_ids, event_types, metadata): (
            Vec<_>,
            Vec<_>,
            Vec<_>,
            Vec<_>,
            Vec<_>,
        ) = events
            .into_iter()
            .map(|e| {
                (
                    e.id.0,
                    e.target_id,
                    e.triggerer_id,
                    e.event_type,
                    e.metadata,
                )
            })
            .multiunzip();
        sqlx::query!(
            "
            INSERT INTO events (
                id,
                target_id.id,
                target_id.type,
                triggerer_id.id,
                triggerer_id.type,
                type,
                metadata
            )
            SELECT * FROM UNNEST (
                $1::bigint[],
                $2::dynamic_id[],
                $3::dynamic_id[],
                $4::event_type[],
                $5::jsonb[]
            )
            ",
            &ids[..],
            &target_ids[..] as &[DynamicId],
            &triggerer_ids[..] as &[Option<DynamicId>],
            &event_types[..] as &[EventType],
            &metadata[..] as &[Option<serde_json::Value>]
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }
}
