use super::models::DatabaseError;
use chrono::Utc;
use dashmap::DashMap;
use deadpool_redis::{Config, Runtime};
use itertools::Itertools;
use redis::{cmd, Cmd, FromRedisValue};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::future::Future;
use std::hash::Hash;

const DEFAULT_EXPIRY: i64 = 1800; // 30 minutes

#[derive(Clone)]
pub struct RedisPool {
    pub pool: deadpool_redis::Pool,
    meta_namespace: String,
}

pub struct RedisConnection {
    pub connection: deadpool_redis::Connection,
    meta_namespace: String,
}

impl RedisPool {
    // initiate a new redis pool
    // testing pool uses a hashmap to mimic redis behaviour for very small data sizes (ie: tests)
    // PANICS: production pool will panic if redis url is not set
    pub fn new(meta_namespace: Option<String>) -> Self {
        let redis_pool = Config::from_url(dotenvy::var("REDIS_URL").expect("Redis URL not set"))
            .builder()
            .expect("Error building Redis pool")
            .max_size(
                dotenvy::var("DATABASE_MAX_CONNECTIONS")
                    .ok()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(10000),
            )
            .runtime(Runtime::Tokio1)
            .build()
            .expect("Redis connection failed");

        RedisPool {
            pool: redis_pool,
            meta_namespace: meta_namespace.unwrap_or("".to_string()),
        }
    }

    pub async fn connect(&self) -> Result<RedisConnection, DatabaseError> {
        Ok(RedisConnection {
            connection: self.pool.get().await?,
            meta_namespace: self.meta_namespace.clone(),
        })
    }
}

impl RedisConnection {
    pub async fn set(
        &mut self,
        namespace: &str,
        id: &str,
        data: &str,
        expiry: Option<i64>,
    ) -> Result<(), DatabaseError> {
        let mut cmd = cmd("SET");
        redis_args(
            &mut cmd,
            vec![
                format!("{}_{}:{}", self.meta_namespace, namespace, id),
                data.to_string(),
                "EX".to_string(),
                expiry.unwrap_or(DEFAULT_EXPIRY).to_string(),
            ]
            .as_slice(),
        );
        redis_execute(&mut cmd, &mut self.connection).await?;
        Ok(())
    }

    pub async fn set_serialized_to_json<Id, D>(
        &mut self,
        namespace: &str,
        id: Id,
        data: D,
        expiry: Option<i64>,
    ) -> Result<(), DatabaseError>
    where
        Id: Display,
        D: serde::Serialize,
    {
        self.set(
            namespace,
            &id.to_string(),
            &serde_json::to_string(&data)?,
            expiry,
        )
        .await
    }

    pub async fn get(
        &mut self,
        namespace: &str,
        id: &str,
    ) -> Result<Option<String>, DatabaseError> {
        let mut cmd = cmd("GET");
        redis_args(
            &mut cmd,
            vec![format!("{}_{}:{}", self.meta_namespace, namespace, id)].as_slice(),
        );
        let res = redis_execute(&mut cmd, &mut self.connection).await?;
        Ok(res)
    }

    pub async fn get_deserialized_from_json<R>(
        &mut self,
        namespace: &str,
        id: &str,
    ) -> Result<Option<R>, DatabaseError>
    where
        R: for<'a> serde::Deserialize<'a>,
    {
        Ok(self
            .get(namespace, id)
            .await?
            .and_then(|x| serde_json::from_str(&x).ok()))
    }

    pub async fn multi_get<R>(
        &mut self,
        namespace: &str,
        ids: impl IntoIterator<Item = impl Display>,
    ) -> Result<Vec<Option<R>>, DatabaseError>
    where
        R: FromRedisValue,
    {
        let mut cmd = cmd("MGET");

        let ids = ids.into_iter().map(|x| x.to_string()).collect_vec();
        redis_args(
            &mut cmd,
            &ids.into_iter()
                .map(|x| format!("{}_{}:{}", self.meta_namespace, namespace, x))
                .collect_vec(),
        );
        Ok(redis_execute(&mut cmd, &mut self.connection).await?)
    }

    pub async fn delete<T1>(&mut self, namespace: &str, id: T1) -> Result<(), DatabaseError>
    where
        T1: Display,
    {
        let mut cmd = cmd("DEL");
        redis_args(
            &mut cmd,
            vec![format!("{}_{}:{}", self.meta_namespace, namespace, id)].as_slice(),
        );
        redis_execute(&mut cmd, &mut self.connection).await?;
        Ok(())
    }

    pub async fn delete_many(
        &mut self,
        iter: impl IntoIterator<Item = (&str, Option<String>)>,
    ) -> Result<(), DatabaseError> {
        let mut cmd = cmd("DEL");
        let mut any = false;
        for (namespace, id) in iter {
            if let Some(id) = id {
                redis_args(
                    &mut cmd,
                    [format!("{}_{}:{}", self.meta_namespace, namespace, id)].as_slice(),
                );
                any = true;
            }
        }

        if any {
            redis_execute(&mut cmd, &mut self.connection).await?;
        }

        Ok(())
    }

    pub async fn get_cached_keys<F, Fut, T, K>(
        &mut self,
        namespace: &str,
        keys: &[K],
        closure: F,
    ) -> Result<Vec<T>, DatabaseError>
    where
        F: FnOnce(Vec<K>) -> Fut,
        Fut: Future<Output = Result<DashMap<K, T>, DatabaseError>>,
        T: Serialize + DeserializeOwned,
        K: Display + Hash + Eq + PartialEq + Clone + DeserializeOwned + Serialize + Debug,
    {
        Ok(self
            .get_cached_keys_raw(namespace, keys, closure)
            .await?
            .into_iter()
            .map(|x| x.1)
            .collect())
    }

    pub async fn get_cached_keys_raw<F, Fut, T, K>(
        &mut self,
        namespace: &str,
        keys: &[K],
        closure: F,
    ) -> Result<HashMap<K, T>, DatabaseError>
    where
        F: FnOnce(Vec<K>) -> Fut,
        Fut: Future<Output = Result<DashMap<K, T>, DatabaseError>>,
        T: Serialize + DeserializeOwned,
        K: Display + Hash + Eq + PartialEq + Clone + DeserializeOwned + Serialize + Debug,
    {
        Ok(self
            .get_cached_keys_raw_with_slug(namespace, None, false, keys, |ids| async move {
                Ok(closure(ids)
                    .await?
                    .into_iter()
                    .map(|(key, val)| (key, (None::<String>, val)))
                    .collect())
            })
            .await?)
    }

    pub async fn get_cached_keys_with_slug<F, Fut, T, I, K, S>(
        &mut self,
        namespace: &str,
        slug_namespace: &str,
        case_sensitive: bool,
        keys: &[I],
        closure: F,
    ) -> Result<Vec<T>, DatabaseError>
    where
        F: FnOnce(Vec<I>) -> Fut,
        Fut: Future<Output = Result<DashMap<K, (Option<S>, T)>, DatabaseError>>,
        T: Serialize + DeserializeOwned,
        I: Display + Hash + Eq + PartialEq + Clone + Debug,
        K: Display + Hash + Eq + PartialEq + Clone + DeserializeOwned + Serialize,
        S: Display + Clone + DeserializeOwned + Serialize + Debug,
    {
        Ok(self
            .get_cached_keys_raw_with_slug(
                namespace,
                Some(slug_namespace),
                case_sensitive,
                keys,
                closure,
            )
            .await?
            .into_iter()
            .map(|x| x.1)
            .collect())
    }

    pub async fn get_cached_keys_raw_with_slug<F, Fut, T, I, K, S>(
        &mut self,
        namespace: &str,
        slug_namespace: Option<&str>,
        case_sensitive: bool,
        keys: &[I],
        closure: F,
    ) -> Result<HashMap<K, T>, DatabaseError>
    where
        F: FnOnce(Vec<I>) -> Fut,
        Fut: Future<Output = Result<DashMap<K, (Option<S>, T)>, DatabaseError>>,
        T: Serialize + DeserializeOwned,
        I: Display + Hash + Eq + PartialEq + Clone + Debug,
        K: Display + Hash + Eq + PartialEq + Clone + DeserializeOwned + Serialize,
        S: Display + Clone + DeserializeOwned + Serialize + Debug,
    {
        let mut ids = keys
            .into_iter()
            .map(|x| (x.to_string(), x.clone()))
            .collect::<HashMap<String, I>>();

        if ids.is_empty() {
            return Ok(HashMap::new());
        }

        println!("{}", ids.len());
        println!("{:?}", ids);

        let slug_ids = if let Some(slug_namespace) = slug_namespace {
            cmd("MGET")
                .arg(
                    ids.iter()
                        .map(|x| {
                            format!(
                                "{}_{slug_namespace}:{}",
                                self.meta_namespace,
                                if case_sensitive {
                                    x.1.to_string()
                                } else {
                                    x.1.to_string().to_lowercase()
                                }
                            )
                        })
                        .collect::<Vec<_>>(),
                )
                .query_async::<_, Vec<Option<String>>>(&mut self.connection)
                .await?
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };

        println!("slug ids: {:?}", slug_ids);

        let mut cached_values = cmd("MGET")
            .arg(
                ids.iter()
                    .map(|x| x.1.to_string())
                    .chain(slug_ids)
                    .map(|x| format!("{}_{namespace}:{x}", self.meta_namespace))
                    .collect::<Vec<_>>(),
            )
            .query_async::<_, Vec<Option<String>>>(&mut self.connection)
            .await?
            .into_iter()
            .filter_map(|x| {
                if let Some(val) =
                    x.and_then(|val| serde_json::from_str::<RedisValue<T, K, S>>(&val).ok())
                {
                    ids.remove(&val.key.to_string());
                    if let Some(ref alias) = val.alias {
                        ids.remove(&alias.to_string());
                    }

                    println!("found: {}", val.key);

                    Some((val.key.clone(), val))
                } else {
                    None
                }
            })
            .collect::<HashMap<_, _>>();

        if !ids.is_empty() {
            // todo: aqquire lock here as well and see if present

            let ids = ids.into_iter().map(|x| x.1).collect::<Vec<_>>();

            let vals = closure(ids).await?;

            if !vals.is_empty() {
                let mut pipe = redis::pipe();

                for (key, (slug, value)) in vals {
                    println!("db found: {} {:?}", key, slug);
                    let value = RedisValue {
                        key: key.clone(),
                        iat: Utc::now().timestamp(),
                        val: value,
                        alias: slug.clone(),
                    };

                    pipe.atomic().set_ex(
                        format!("{}_{namespace}:{key}", self.meta_namespace),
                        serde_json::to_string(&value)?,
                        DEFAULT_EXPIRY as u64,
                    );

                    if let Some(slug) = slug {
                        if let Some(slug_namespace) = slug_namespace {
                            pipe.atomic().set_ex(
                                format!(
                                    "{}_{slug_namespace}:{}",
                                    self.meta_namespace,
                                    if case_sensitive {
                                        slug.to_string()
                                    } else {
                                        slug.to_string().to_lowercase()
                                    }
                                ),
                                key.to_string(),
                                DEFAULT_EXPIRY as u64,
                            );
                        }
                    }

                    cached_values.insert(key, value);
                }

                pipe.query_async(&mut self.connection).await?;
            }
        }

        Ok(cached_values.into_iter().map(|x| (x.0, x.1.val)).collect())
    }
}

#[derive(Serialize, Deserialize)]
pub struct RedisValue<T, K, S> {
    key: K,
    #[serde(skip_serializing_if = "Option::is_none")]
    alias: Option<S>,
    iat: i64,
    val: T,
}

pub fn redis_args(cmd: &mut Cmd, args: &[String]) {
    for arg in args {
        cmd.arg(arg);
    }
}

pub async fn redis_execute<T>(
    cmd: &mut Cmd,
    redis: &mut deadpool_redis::Connection,
) -> Result<T, deadpool_redis::PoolError>
where
    T: redis::FromRedisValue,
{
    let res = cmd.query_async::<_, T>(redis).await?;
    Ok(res)
}
