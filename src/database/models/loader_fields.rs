use crate::database::redis::RedisPool;
use crate::models::ids::base62_impl::parse_base62;
use crate::routes::ApiError;
use crate::routes::v3::project_creation::CreateError;

use super::ids::*;
use super::DatabaseError;
use chrono::DateTime;
use chrono::Utc;
use futures::TryStreamExt;
use itertools::Itertools;
use serde::{Deserialize, Serialize};
use serde_json::json;

const GAME_LOADERS_NAMESPACE: &str = "game_loaders";
const LOADER_FIELD_ID_NAMESPACE: &str = "loader_field_ids"; // from str to id
const LOADER_FIELDS_NAMESPACE: &str = "loader_fields";
const LOADER_FIELD_ENUMS_NAMESPACE: &str = "loader_field_enums";
const VERSION_FIELDS_NAMESPACE: &str = "version_fields_enums";

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Game {
    MinecraftJava,
    MinecraftBedrock
}

impl Game {
    pub fn name(&self) -> &'static str {
        match self {
            Game::MinecraftJava => "minecraft-java",
            Game::MinecraftBedrock => "minecraft-bedrock"
        }
    }

    pub async fn get_id<'a, E>(name: &str, exec: E) -> Result<Option<GameId>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT id FROM games
            WHERE name = $1
            ",
            name
        )
        .fetch_optional(exec)
        .await?;

        Ok(result.map(|r| GameId(r.id)))
    }

}

#[derive(Serialize, Deserialize)]
pub struct Loader {
    pub id: LoaderId,
    pub loader: String,
    pub icon: String,
    pub supported_project_types: Vec<String>,
}

impl Loader {
    pub async fn get_id<'a, E>(name: &str, exec: E) -> Result<Option<LoaderId>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT id FROM loaders
            WHERE loader = $1
            ",
            name
        )
        .fetch_optional(exec)
        .await?;

        Ok(result.map(|r| LoaderId(r.id)))
    }

    pub async fn list<'a, E>(game_name_or_id : &str , exec: E, redis: &RedisPool) -> Result<Vec<Loader>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT l.id id, l.loader loader, l.icon icon,
            ARRAY_AGG(DISTINCT pt.name) filter (where pt.name is not null) project_types
            FROM loaders l
            INNER JOIN games g ON l.game_id = g.id
            LEFT OUTER JOIN loaders_project_types lpt ON joining_loader_id = l.id
            LEFT OUTER JOIN project_types pt ON lpt.joining_project_type_id = pt.id
            WHERE g.name = $1
            GROUP BY l.id;
            ",
            game_name_or_id,
        )
        .fetch_many(exec)
        .try_filter_map(|e| async {
            Ok(e.right().map(|x| Loader {
                id: LoaderId(x.id),
                loader: x.loader,
                icon: x.icon,
                supported_project_types: x
                    .project_types
                    .unwrap_or_default()
                    .iter()
                    .map(|x| x.to_string())
                    .collect(),
            }))
        })
        .try_collect::<Vec<_>>()
        .await?;
        println!("Just collected loaders for game {}, got {} loaders", game_name_or_id, result.len());
        Ok(result)
    }

    pub async fn list_id<'a, E>(game_id : GameId , exec: E, redis: &RedisPool) -> Result<Vec<Loader>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT l.id id, l.loader loader, l.icon icon,
            ARRAY_AGG(DISTINCT pt.name) filter (where pt.name is not null) project_types
            FROM loaders l
            INNER JOIN games g ON l.game_id = g.id
            LEFT OUTER JOIN loaders_project_types lpt ON joining_loader_id = l.id
            LEFT OUTER JOIN project_types pt ON lpt.joining_project_type_id = pt.id
            WHERE g.id = $1
            GROUP BY l.id;
            ",
            game_id.0,
        )
        .fetch_many(exec)
        .try_filter_map(|e| async {
            Ok(e.right().map(|x| Loader {
                id: LoaderId(x.id),
                loader: x.loader,
                icon: x.icon,
                supported_project_types: x
                    .project_types
                    .unwrap_or_default()
                    .iter()
                    .map(|x| x.to_string())
                    .collect(),
            }))
        })
        .try_collect::<Vec<_>>()
        .await?;
        println!("Just collected loaders for game {}, got {} loaders", game_id.0, result.len());
        Ok(result)
    }
}


#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LoaderField {
    pub id: LoaderFieldId,
    pub loader_id: LoaderId,
    pub loader_name : String,
    pub field: String,
    pub field_type: LoaderFieldType,
    pub optional: bool,
    pub min_val: Option<i32>,
    pub max_val: Option<i32>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum LoaderFieldType {
    Integer,
    Text,
    Enum(LoaderFieldEnumId),
    Boolean,
    ArrayInteger,
    ArrayText,
    ArrayEnum(LoaderFieldEnumId),
    ArrayBoolean,
    Unknown
}
impl LoaderFieldType {
    pub fn build(field_name : &str, loader_field_enum : Option<i32>) -> LoaderFieldType {
        match (field_name, loader_field_enum) {
            ("integer", _) => LoaderFieldType::Integer,
            ("text", _) => LoaderFieldType::Text,
            ("boolean", _) => LoaderFieldType::Boolean,
            ("array_integer", _) => LoaderFieldType::ArrayInteger,
            ("array_text", _) => LoaderFieldType::ArrayText,
            ("array_boolean", _) => LoaderFieldType::ArrayBoolean,
            ("enum", Some(id)) => LoaderFieldType::Enum(LoaderFieldEnumId(id)),
            ("array_enum", Some(id)) => LoaderFieldType::ArrayEnum(LoaderFieldEnumId(id)),
            _ => LoaderFieldType::Unknown
        }
    }

    pub fn to_str(&self) -> &'static str {
        match self {
            LoaderFieldType::Integer => "integer",
            LoaderFieldType::Text => "text",
            LoaderFieldType::Boolean => "boolean",
            LoaderFieldType::ArrayInteger => "array_integer",
            LoaderFieldType::ArrayText => "array_text",
            LoaderFieldType::ArrayBoolean => "array_boolean",
            LoaderFieldType::Enum(_) => "enum",
            LoaderFieldType::ArrayEnum(_) => "array_enum",
            LoaderFieldType::Unknown => "unknown"
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LoaderFieldEnum {
    pub id: LoaderFieldEnumId,
    pub game_id: GameId,
    pub enum_name: String,
    pub ordering: Option<i32>,
    pub hidable: bool,
}


#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct LoaderFieldEnumValue {
    pub id: LoaderFieldEnumValueId,
    pub enum_id: LoaderFieldEnumId,
    pub value: String,
    pub ordering: Option<i32>,
    pub created: DateTime<Utc>,
    pub metadata: serde_json::Value,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct VersionField {
    pub version_id: VersionId,
    pub field_id: LoaderFieldId,
    pub loader_name: String,
    pub field_name: String,
    pub value: VersionFieldValue,
}
impl VersionField {
    pub async fn insert_many(
        items: Vec<Self>,
        transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ) -> Result<(), DatabaseError> {
         let mut query_version_fields = vec![];
         for item in items {
            let base = QueryVersionField {
                version_id: item.version_id,
                field_id: item.field_id,
                int_value: None,
                enum_value: None,
                string_value: None,
            };

            match item.value {
                VersionFieldValue::Integer(i) => query_version_fields.push(base.clone().with_int_value(i)),
                VersionFieldValue::Text(s) => query_version_fields.push(base.clone().with_string_value(s)),
                VersionFieldValue::Boolean(b) => query_version_fields.push(base.clone().with_int_value(if b { 1 } else { 0 })),
                VersionFieldValue::ArrayInteger(v) => {
                    for i in v {
                        query_version_fields.push(base.clone().with_int_value(i));
                    }
                }
                VersionFieldValue::ArrayText(v) => {
                    for s in v {
                        query_version_fields.push(base.clone().with_string_value(s));
                    }
                }
                VersionFieldValue::ArrayBoolean(v) => {
                    for b in v {
                        query_version_fields.push(base.clone().with_int_value(if b { 1 } else { 0 }));
                    }
                }
                VersionFieldValue::Enum(_, v) => query_version_fields.push(base.clone().with_enum_value(v)),
                VersionFieldValue::ArrayEnum(_, v) => {
                    for ev in v {
                        query_version_fields.push(base.clone().with_enum_value(ev));
                    }
                }
                VersionFieldValue::Unknown => {}
            };
         }

            let (field_ids, version_ids, int_values, enum_values, string_values): (Vec<_>, Vec<_>, Vec<_>, Vec<_>, Vec<_>) = query_version_fields
                .iter()
                .map(|l| (l.field_id.0, l.version_id.0, l.int_value, l.enum_value.as_ref().map(|e|e.id.0), l.string_value.clone()))
                .multiunzip();

            sqlx::query!(
                "
                INSERT INTO version_fields (field_id, version_id, int_value, string_value, enum_value)
                SELECT * FROM UNNEST($1::integer[], $2::bigint[], $3::integer[], $4::text[], $5::integer[])
                ",
                &field_ids[..],
                &version_ids[..],
                &int_values[..] as &[Option<i32>],
                &string_values[..] as &[Option<String>],
                &enum_values[..] as &[Option<i32>]
            )
            .execute(&mut *transaction)
            .await?;
    
            Ok(())
    }
    
    pub async fn check_parse<'a, E>(version_id : VersionId, loader_field : LoaderField, key : &str, value : serde_json::Value, exec : E, redis : &RedisPool) -> Result<VersionField, CreateError> 
    where E : sqlx::Executor<'a, Database = sqlx::Postgres>
    {
        let value = VersionFieldValue::parse(&loader_field, value, exec, &redis).await?;

        Ok(VersionField {
            version_id,
            field_id: loader_field.id,
            loader_name: loader_field.loader_name,
            field_name: loader_field.field,
            value
        })
    }

    pub fn build(loader_field : LoaderField, version_id : VersionId, query_version_fields : Vec<QueryVersionField>) ->  Result<VersionField, DatabaseError> {
        let value = VersionFieldValue::build(&loader_field.field_type, query_version_fields)?;
        Ok(VersionField {
            version_id,
            field_id: loader_field.id,
            loader_name: loader_field.loader_name,
            field_name: loader_field.field,
            value
        })
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum VersionFieldValue {
    Integer(i32),
    Text(String),
    Enum(LoaderFieldEnumId, LoaderFieldEnumValue),
    Boolean(bool),
    ArrayInteger(Vec<i32>),
    ArrayText(Vec<String>),
    ArrayEnum(LoaderFieldEnumId, Vec<LoaderFieldEnumValue>),
    ArrayBoolean(Vec<bool>),
    Unknown
}
impl VersionFieldValue {
    // TODO: this could be combined with build
    pub async fn parse<'a, E>(loader_field: &LoaderField, value : serde_json::Value, exec : E, redis : &RedisPool) -> Result<VersionFieldValue, CreateError>
    where E : sqlx::Executor<'a, Database = sqlx::Postgres>
    
     {
        let field_name = &loader_field.field;
        let field_type = &loader_field.field_type;

        let incorrect_type_error = |field_type : &str| CreateError::InvalidInput(
            format!("Provided value for {field_name} could not be parsed to {field_type} "));

            // Todo more efficient?
        let enum_array = if let LoaderFieldType::Enum(id) | LoaderFieldType::ArrayEnum(id) = field_type {
            LoaderFieldEnumValue::list(*id, exec, redis).await?
        } else {
            vec![]
        };

        Ok(match field_type {
            LoaderFieldType::Integer => VersionFieldValue::Integer(
                    serde_json::from_value(value).map_err(|_| incorrect_type_error("integer"))?
            ),
            LoaderFieldType::Text => VersionFieldValue::Text(
                value.as_str().ok_or_else(|| incorrect_type_error("string"))?.to_string()
            ),
            LoaderFieldType::Boolean => VersionFieldValue::Boolean(
                    value.as_bool().ok_or_else(|| incorrect_type_error("boolean"))?
            ),
            LoaderFieldType::ArrayInteger => VersionFieldValue::ArrayInteger(
{
                let array_values : Vec<i32> = serde_json::from_value(value).map_err(|_| incorrect_type_error("array of integers"))?;
                array_values.into_iter().map(|v| v).collect()   
}            ),
            LoaderFieldType::ArrayText => VersionFieldValue::ArrayText(
                {
                    let array_values : Vec<String> = serde_json::from_value(value).map_err(|_| incorrect_type_error("array of strings"))?;
                    array_values.into_iter().map(|v| v.to_string()).collect()
                }
            ),
            LoaderFieldType::ArrayBoolean => VersionFieldValue::ArrayBoolean(
                {
                    let array_values : Vec<i64> = serde_json::from_value(value).map_err(|_| incorrect_type_error("array of booleans"))?;
                    array_values.into_iter().map(|v| v != 0).collect()
                }
            ),
            LoaderFieldType::Enum(id) => VersionFieldValue::Enum(*id,
                {
                    let enum_value = value.as_str().ok_or_else(|| incorrect_type_error("enum"))?;
                    if let Some(ev) = enum_array.into_iter().find(|v| v.value == enum_value) {
                        ev
                    } else {
                        return Err(CreateError::InvalidInput(format!("Provided value '{enum_value}' is not a valid variant for {field_name}")));
                    }
                }
            ),
            LoaderFieldType::ArrayEnum(id) => VersionFieldValue::ArrayEnum(*id,
                {
                    let array_values : Vec<String> = serde_json::from_value(value).map_err(|_| incorrect_type_error("array of enums"))?;
                    let mut enum_values = vec![];
                    for av in array_values {
                        if let Some(ev) = enum_array.iter().find(|v| v.value == av) {
                            enum_values.push(ev.clone());
                        } else {
                            return Err(CreateError::InvalidInput(format!("Provided value '{av}' is not a valid variant for {field_name}")));
                        }
                    }
                    enum_values                    
                }
            ),
            LoaderFieldType::Unknown => VersionFieldValue::Unknown
        })
    }

    pub fn build(field_type : &LoaderFieldType, qvfs : Vec<QueryVersionField>) -> Result<VersionFieldValue, DatabaseError> {
        let field_name = field_type.to_str();
        // TODO: should not use numbers , should use id with tostring
        let get_first = |qvfs: Vec<QueryVersionField>| -> Result<QueryVersionField, DatabaseError> {
            if qvfs.len() > 1 {
                return Err(DatabaseError::SchemaError(
                    format!("Multiple fields for field {}", field_name)
                ));
            }
            Ok(qvfs.into_iter().next().ok_or_else(|| DatabaseError::SchemaError(
                format!("No version fields for field {}", field_name)
            ))?)
        };

        // TODO: should not use numbers , should use id with tostring
        let did_not_exist_error = |field_name : &str, desired_field : &str| DatabaseError::SchemaError(
            format!("Field name {} for field {} in does not exist",  desired_field , field_name));

        Ok(match field_type {
            LoaderFieldType::Integer => VersionFieldValue::Integer(
                get_first(qvfs)?.int_value.ok_or(did_not_exist_error(field_name, "int_value"))?
            ),
            LoaderFieldType::Text => VersionFieldValue::Text(
                get_first(qvfs)?.string_value.ok_or(did_not_exist_error( field_name, "string_value"))?
            ),
            LoaderFieldType::Boolean => VersionFieldValue::Boolean(
                get_first(qvfs)?.int_value.ok_or(did_not_exist_error(field_name, "int_value"))? != 0
            ),
            LoaderFieldType::ArrayInteger => VersionFieldValue::ArrayInteger(
                qvfs.into_iter().map(|qvf|
                    Ok::<i32,DatabaseError>(qvf.int_value.ok_or(did_not_exist_error(field_name, "int_value"))?)).collect::<Result<_,_>>()?
            ),
            LoaderFieldType::ArrayText => VersionFieldValue::ArrayText(
                qvfs.into_iter().map(|qvf|
                    Ok::<String,DatabaseError>(qvf.string_value.ok_or(did_not_exist_error( field_name, "string_value"))?)).collect::<Result<_,_>>()?
            ),
            LoaderFieldType::ArrayBoolean => VersionFieldValue::ArrayBoolean(
                qvfs.into_iter().map(|qvf|
                    Ok::<bool,DatabaseError>(qvf.int_value.ok_or(did_not_exist_error( field_name, "int_value"))? != 0)).collect::<Result<_,_>>()?
            ),

            LoaderFieldType::Enum(id) => VersionFieldValue::Enum(*id, 
                get_first(qvfs)?.enum_value.ok_or(did_not_exist_error( field_name, "enum_value"))?
            ),
            LoaderFieldType::ArrayEnum(id) => VersionFieldValue::ArrayEnum(*id, 
                qvfs.into_iter().map(|qvf|
                    Ok::<LoaderFieldEnumValue,DatabaseError>(qvf.enum_value.ok_or(did_not_exist_error( field_name, "enum_value"))?)).collect::<Result<_,_>>()?
            ),
            LoaderFieldType::Unknown => VersionFieldValue::Unknown
        })
    }

    pub fn serialize_internal(&self) -> serde_json::Value {
        // Serialize to internal value
        match self {
            VersionFieldValue::Integer(i) => serde_json::Value::Number((*i).into()),
            VersionFieldValue::Text(s) => serde_json::Value::String(s.clone()),
            VersionFieldValue::Boolean(b) => serde_json::Value::Bool(*b),
            VersionFieldValue::ArrayInteger(v) => serde_json::Value::Array(v.iter().map(|i| serde_json::Value::Number((*i).into())).collect()),
            VersionFieldValue::ArrayText(v) => serde_json::Value::Array(v.iter().map(|s| serde_json::Value::String(s.clone())).collect()),
            VersionFieldValue::ArrayBoolean(v) => serde_json::Value::Array(v.iter().map(|b| serde_json::Value::Bool(*b)).collect()),
            VersionFieldValue::Enum(_, v) => serde_json::Value::String(v.value.clone()),
            VersionFieldValue::ArrayEnum(_, v) => serde_json::Value::Array(v.iter().map(|v| serde_json::Value::String(v.value.clone())).collect()),
            VersionFieldValue::Unknown => serde_json::Value::Null
        }
    }
}


#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct QueryVersionField {
    pub version_id: VersionId,
    pub field_id: LoaderFieldId,
    pub int_value: Option<i32>,
    pub enum_value: Option<LoaderFieldEnumValue>,
    pub string_value: Option<String>,
}

impl QueryVersionField {
    pub fn with_int_value(mut self, int_value: i32) -> Self {
        self.int_value = Some(int_value);
        self
    }

    pub fn with_enum_value(mut self, enum_value: LoaderFieldEnumValue) -> Self {
        self.enum_value = Some(enum_value);
        self
    }

    pub fn with_string_value(mut self, string_value: String) -> Self {
        self.string_value = Some(string_value);
        self
    }
}


#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SideType {
    pub id: SideTypeId,
    pub name: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct GameVersion {
    pub id: LoaderFieldEnumValueId,
    pub version: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub created: DateTime<Utc>,
    pub major: bool,
}

// game version from loaderfieldenumvalue
// TODO: remove, after moving gameversion to legacy minecraft
impl GameVersion {
    fn from(game_version: LoaderFieldEnumValue) -> Result<Self, DatabaseError> {
        // TODO: should not use numbers , should use id with tostring
        let version_type = game_version.metadata.get("type").map(|x| x.as_str()).flatten().ok_or_else(|| format!("Could not read GameVersion {}: Missing version type", game_version.id.0)).unwrap_or_default().to_string();
        let major = game_version.metadata.get("major").map(|x| x.as_bool()).flatten().ok_or_else(|| format!("Could not read GameVersion {}: Missing version major", game_version.id.0)).unwrap_or_default();

        Ok(Self {
            id: game_version.id,
            version: game_version.value,
            type_: version_type,
            created: game_version.created,
            major,
        })
    }
}

impl LoaderField {

    pub async fn get_field<'a, E>(
        field : &str,
        loader_id: LoaderId,
        exec: E,
    ) -> Result<Option<LoaderField>, DatabaseError>
    where 
    E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {    
        let fields = Self::get_fields(field, &[loader_id], exec).await?;
        Ok(fields.into_iter().next())
    }

    pub async fn get_fields<'a, E>(
        field : &str,
        loader_ids : &[LoaderId],
        exec: E,
    ) -> Result<Vec<LoaderField>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
     {    
        let result = sqlx::query!(
            "
            SELECT lf.id, lf.loader_id, lf.field, lf.field_type, lf.optional, lf.min_val, lf.max_val, lf.enum_type, l.loader
            FROM loader_fields lf
            INNER JOIN loaders l ON lf.loader_id = l.id
            WHERE loader_id = ANY($1) AND field = $2
            ",
            &loader_ids.into_iter().map(|l|l.0).collect::<Vec<i32>>(),
            field
        )
        .fetch_many(exec)
        .try_filter_map(|e| async { Ok(e.right().map(|r| 
            LoaderField {
                id: LoaderFieldId(r.id),
                loader_id: LoaderId(r.loader_id),
                field: r.field,
                field_type: LoaderFieldType::build(&r.field_type, r.enum_type),
                loader_name: r.loader,
                optional: r.optional,
                min_val: r.min_val,
                max_val: r.max_val
            }
        )) })
        .try_collect::<Vec<LoaderField>>()
        .await?;

        Ok(result)
    }
}

impl LoaderFieldEnum {
    pub async fn get<'a, E>(enum_name : &str, game_name : &str, exec: E, redis: &RedisPool) -> Result<Option<LoaderFieldEnum>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {
        let result = sqlx::query!(
            "
            SELECT lfe.id, lfe.game_id, lfe.enum_name, lfe.ordering, lfe.hidable 
            FROM loader_field_enums lfe
            INNER JOIN games g ON lfe.game_id = g.id
            WHERE g.name = $1 AND lfe.enum_name = $2
            ",
            game_name,
            enum_name
        )
        .fetch_optional(exec).await?;


        Ok(result.map(|l| LoaderFieldEnum {
            id: LoaderFieldEnumId(l.id),
            game_id: GameId(l.game_id),
            enum_name: l.enum_name,
            ordering: l.ordering,
            hidable: l.hidable,
         }
        )) 
    }
}

impl LoaderFieldEnumValue {
    pub async fn list<'a, E>(loader_field_enum_id : LoaderFieldEnumId, exec: E, redis: &RedisPool) -> Result<Vec<LoaderFieldEnumValue>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {

        let result = sqlx::query!(
            "
            SELECT id, enum_id, value, ordering, metadata, created FROM loader_field_enum_values
            WHERE enum_id = $1
            ", 
            loader_field_enum_id.0
        )
        .fetch_many(exec)
        .try_filter_map(|e| async { Ok(e.right().map(|c| 
            LoaderFieldEnumValue {
                id: LoaderFieldEnumValueId(c.id),
                enum_id: LoaderFieldEnumId(c.enum_id),
                value: c.value,
                ordering: c.ordering,
                created: c.created,
                metadata: c.metadata.unwrap_or_default()
            }
        )) })
        .try_collect::<Vec<LoaderFieldEnumValue>>()
        .await?;

        Ok(result)
    }

    // Matches filter against metadata of enum values
    pub async fn list_filter<'a, E>(
        loader_field_enum_id : LoaderFieldEnumId,
        filter : serde_json::Value,
        exec: E,
        redis: &RedisPool,
    ) -> Result<Vec<LoaderFieldEnumValue>, DatabaseError>
    where
        E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    {

        let filter = filter.as_object().ok_or(DatabaseError::SchemaError("Filter must be an object".to_string()))?;
        
        let result = Self::list(loader_field_enum_id, exec, redis)
            .await?
            .into_iter()
            .filter(|x| {
                let mut bool = true;
                for (key, value) in filter {
                    if let Some(metadata_value) = x.metadata.get(key) {
                        bool &= metadata_value == value;
                    } else {
                        bool = false;
                    }
                }
                bool
            })
            .collect();

        Ok(result)
    }

}

#[derive(Default)]
pub struct GameVersionBuilder<'a> {
    pub version: Option<&'a str>,
    pub version_type: Option<&'a str>,
    pub date: Option<&'a DateTime<Utc>>,
}

impl<'a> GameVersionBuilder<'a> {
    pub fn new() -> Self {
        Self::default()
    }
        /// The game version.  Spaces must be replaced with '_' for it to be valid
        pub fn version(self, version: &'a str) -> Result<GameVersionBuilder<'a>, DatabaseError> {
            Ok(Self {
                version: Some(version),
                ..self
            })
        }
    
        pub fn version_type(
            self,
            version_type: &'a str,
        ) -> Result<GameVersionBuilder<'a>, DatabaseError> {
            Ok(Self {
                version_type: Some(version_type),
                ..self
            })
        }
    
        pub fn created(self, created: &'a DateTime<Utc>) -> GameVersionBuilder<'a> {
            Self {
                date: Some(created),
                ..self
            }
        }
    
        pub async fn insert<'b, E>(self, exec: E, redis: &RedisPool) -> Result<GameVersionId, DatabaseError>
        where
            E: sqlx::Executor<'b, Database = sqlx::Postgres> + Copy
        {
            // TODO: this is hardcoded for minecraft-java
            let game_name = Game::MinecraftJava.name();
            let game_versions_enum = LoaderFieldEnum::get("game_versions", game_name, exec, redis).await?
                .ok_or(DatabaseError::SchemaError("Missing loaders field: 'game_versions'".to_string()))?;
            
            
            // Get enum id for game versions
            let metadata = json!({
                "type": self.version_type,
                "major": false
            });

            // This looks like a mess, but it *should* work
            // This allows game versions to be partially updated without
            // replacing the unspecified fields with defaults.
            let result = sqlx::query!(
                "
                INSERT INTO loader_field_enum_values (enum_id, value, created, metadata)
                VALUES ($1, $2, COALESCE($3, timezone('utc', now())), $4)
                ON CONFLICT (enum_id, value) DO UPDATE
                    SET metadata = COALESCE($4, loader_field_enum_values.metadata),
                        created = COALESCE($3, loader_field_enum_values.created)
                RETURNING id
                ",
                game_versions_enum.id.0,
                self.version,
                self.date.map(chrono::DateTime::naive_utc),
                metadata
            )
            .fetch_one(exec)
            .await?;

            Ok(GameVersionId(result.id))
        }
    
}

impl GameVersion {
    pub fn builder() -> GameVersionBuilder<'static> {
        GameVersionBuilder::default()
    }

    // pub async fn get_id<'a, E>(
    //     version: &str,
    //     exec: E,
    // ) -> Result<Option<GameVersionId>, DatabaseError>
    // where
    //     E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    // {
    //     let result = sqlx::query!(
    //         "
    //         SELECT id FROM game_versions
    //         WHERE version = $1
    //         ",
    //         version
    //     )
    //     .fetch_optional(exec)
    //     .await?;

    //     Ok(result.map(|r| GameVersionId(r.id)))
    // }

    // pub async fn list<'a, E>(exec: E, redis: &RedisPool) -> Result<Vec<GameVersion>, DatabaseError>
    // where
    //     E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    // {
    //     let result = sqlx::query!(
    //         "
    //         SELECT 
    //         SELECT gv.id id, gv.version version_, gv.type type_, gv.created created, gv.major FROM game_versions gv
    //         ORDER BY created DESC
    //         "
    //     )
    //     .fetch_many(exec)
    //     .try_filter_map(|e| async { Ok(e.right().map(|c| GameVersion {
    //         id: GameVersionId(c.id),
    //         version: c.version_,
    //         type_: c.type_,
    //         created: c.created,
    //         major: c.major
    //     })) })
    //     .try_collect::<Vec<GameVersion>>()
    //     .await?;

    //     Ok(result)
    // }

    // pub async fn list_filter<'a, E>(
    //     version_type_option: Option<&str>,
    //     major_option: Option<bool>,
    //     exec: E,
    //     redis: &RedisPool,
    // ) -> Result<Vec<GameVersion>, DatabaseError>
    // where
    //     E: sqlx::Executor<'a, Database = sqlx::Postgres>,
    // {
    //     let result = Self::list(exec, redis)
    //         .await?
    //         .into_iter()
    //         .filter(|x| {
    //             let mut bool = true;

    //             if let Some(version_type) = version_type_option {
    //                 bool &= &*x.type_ == version_type;
    //             }
    //             if let Some(major) = major_option {
    //                 bool &= x.major == major;
    //             }

    //             bool
    //         })
    //         .collect();

    //     Ok(result)
    // }
}

