use super::{
    dynamic::{DynamicId, IdType},
    generate_event_id, DatabaseError, EventId, OrganizationId, ProjectId, UserId,
};
use chrono::{DateTime, Utc};
use itertools::Itertools;
use sqlx::postgres::{PgHasArrayType, PgTypeInfo};
use std::convert::{TryFrom, TryInto};

#[derive(sqlx::Type, Clone, Copy)]
#[sqlx(type_name = "text")]
#[sqlx(rename_all = "snake_case")]
pub enum EventType {
    ProjectCreated,
}

impl PgHasArrayType for EventType {
    fn array_type_info() -> sqlx::postgres::PgTypeInfo {
        PgTypeInfo::with_name("_text")
    }
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
    pub id: EventId,
    pub event_data: EventData,
    pub time: DateTime<Utc>,
}

struct RawEvent {
    pub id: EventId,
    pub target_id: i64,
    pub target_id_type: IdType,
    pub triggerer_id: Option<i64>,
    pub triggerer_id_type: Option<IdType>,
    pub event_type: EventType,
    pub metadata: Option<serde_json::Value>,
    pub created: Option<DateTime<Utc>>,
}

pub struct EventSelector {
    pub id: DynamicId,
    pub event_type: EventType,
}

impl Event {
    pub async fn new(
        event_data: EventData,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<Self, DatabaseError> {
        let id = generate_event_id(transaction).await?;
        Ok(Self {
            id,
            event_data,
            time: Default::default(),
        })
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
            } => {
                let target_id = DynamicId::from(project_id);
                let triggerer_id = DynamicId::from(creator_id);
                RawEvent {
                    id: value.id,
                    target_id: target_id.id,
                    target_id_type: target_id.id_type,
                    triggerer_id: Some(triggerer_id.id),
                    triggerer_id_type: Some(triggerer_id.id_type),
                    event_type: EventType::ProjectCreated,
                    metadata: None,
                    created: None,
                }
            }
        }
    }
}

impl TryFrom<RawEvent> for Event {
    type Error = DatabaseError;

    fn try_from(value: RawEvent) -> Result<Self, Self::Error> {
        let target_id = DynamicId {
            id: value.target_id,
            id_type: value.target_id_type,
        };
        let triggerer_id = match (value.triggerer_id, value.triggerer_id_type) {
            (Some(id), Some(id_type)) => Some(DynamicId { id, id_type }),
            _ => None,
        };
        Ok(match value.event_type {
            EventType::ProjectCreated => Event {
                id: value.id,
                event_data: EventData::ProjectCreated {
                    project_id: target_id.try_into()?,
                    creator_id: triggerer_id.map_or_else(|| {
                        Err(DatabaseError::UnexpectedNull(
                            "Neither triggerer_id nor triggerer_id_type should be null for project creation".to_string(),
                        ))
                    }, |v| v.try_into())?,
                },
                time: value.created.map_or_else(
                    || {
                        Err(DatabaseError::UnexpectedNull(
                            "the value of created should not be null".to_string(),
                        ))
                    },
                    |c| Ok(c),
                )?,
            },
        })
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

    pub async fn get_events(
        target_selectors: &[EventSelector],
        triggerer_selectors: &[EventSelector],
        exec: impl sqlx::Executor<'_, Database = sqlx::Postgres>,
    ) -> Result<Vec<Event>, DatabaseError> {
        let (target_ids, target_id_types, target_event_types) =
            unzip_event_selectors(target_selectors);
        let (triggerer_ids, triggerer_id_types, triggerer_event_types) =
            unzip_event_selectors(triggerer_selectors);
        Ok(sqlx::query_as!(
            RawEvent,
            r#"
            SELECT 
                id,
                target_id,
                target_id_type as "target_id_type: _",
                triggerer_id,
                triggerer_id_type as "triggerer_id_type: _",
                event_type as "event_type: _",
                metadata,
                created
            FROM events e
            WHERE 
                (target_id, target_id_type, event_type) 
                = ANY(SELECT * FROM UNNEST ($1::bigint[], $2::text[], $3::text[]))
            OR
                (triggerer_id, triggerer_id_type, event_type) 
                = ANY(SELECT * FROM UNNEST ($4::bigint[], $5::text[], $6::text[]))
            ORDER BY created DESC
            "#,
            &target_ids[..],
            &target_id_types[..] as &[IdType],
            &target_event_types[..] as &[EventType],
            &triggerer_ids[..],
            &triggerer_id_types[..] as &[IdType],
            &triggerer_event_types[..] as &[EventType]
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
        let (
            ids,
            target_ids,
            target_id_types,
            triggerer_ids,
            triggerer_id_types,
            event_types,
            metadata,
        ): (Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>) = events
            .into_iter()
            .map(|e| {
                (
                    e.id.0,
                    e.target_id,
                    e.target_id_type,
                    e.triggerer_id,
                    e.triggerer_id_type,
                    e.event_type,
                    e.metadata,
                )
            })
            .multiunzip();
        sqlx::query!(
            "
            INSERT INTO events (
                id,
                target_id,
                target_id_type,
                triggerer_id,
                triggerer_id_type,
                event_type,
                metadata
            )
            SELECT * FROM UNNEST (
                $1::bigint[],
                $2::bigint[],
                $3::text[],
                $4::bigint[],
                $5::text[],
                $6::text[],
                $7::jsonb[]
            )
            ",
            &ids[..],
            &target_ids[..],
            &target_id_types[..] as &[IdType],
            &triggerer_ids[..] as &[Option<i64>],
            &triggerer_id_types[..] as &[Option<IdType>],
            &event_types[..] as &[EventType],
            &metadata[..] as &[Option<serde_json::Value>]
        )
        .execute(&mut **transaction)
        .await?;

        Ok(())
    }
}

fn unzip_event_selectors(
    target_selectors: &[EventSelector],
) -> (Vec<i64>, Vec<IdType>, Vec<EventType>) {
    target_selectors
        .into_iter()
        .map(|t| (t.id.id, t.id.id_type, t.event_type))
        .multiunzip()
}
