use std::fmt::Display;
use deadpool_redis::{Runtime, Config};
use redis::{cmd, ToRedisArgs, FromRedisValue};
use super::models::DatabaseError;

const DEFAULT_EXPIRY : i64 = 1800; // 30 minutes

#[derive(Clone)]
pub struct RedisPool {
    pool : deadpool_redis::Pool,
    meta_namespace : String
}

impl RedisPool {

    // initiate a new redis pool
    // testing pool uses a hashmap to mimic redis behaviour for very small data sizes (ie: tests)
    // PANICS: production pool will panic if redis url is not set
    pub fn new(meta_namespace : Option<String>) -> Self {
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
            pool : redis_pool,
            meta_namespace: meta_namespace.unwrap_or("".to_string())
        }
}

    pub async fn set<T1, T2>(&self, namespace : &str, id : T1, data : T2, expiry : Option<i64>) -> Result<(), DatabaseError> 
    where T1 : Display,
          T2 : ToRedisArgs
    {
        let mut redis_connection = self.pool.get().await?;

        println!("SET: {}_{}:{}", self.meta_namespace, namespace, id);
        cmd("SET")
            .arg(format!("{}_{}:{}", self.meta_namespace, namespace, id))
            .arg(data)
            .arg("EX")
            .arg(expiry.unwrap_or(DEFAULT_EXPIRY))
            .query_async::<_, ()>(&mut redis_connection)
            .await?;

        Ok(())
    } 

    pub async fn get<R,T1>(&self, namespace : &str, id : T1) -> Result<Option<R>, DatabaseError>  
    where T1 : Display,
        R: FromRedisValue
    {
        let mut redis_connection = self.pool.get().await?;

        println!("GET: {}_{}:{}", self.meta_namespace, namespace, id);
        let res = cmd("GET")
            .arg(format!("{}_{}:{}", self.meta_namespace, namespace, id))
            .query_async::<_, Option<R>>(&mut redis_connection)
            .await?;
        Ok(res)
    }

    pub async fn multi_get<R,T1>(&self, namespace : &str, ids : Vec<T1>) -> Result<Vec<Option<R>>, DatabaseError>  
    where T1 : Display,
        R: FromRedisValue
    {
        let mut redis_connection = self.pool.get().await?;

        for id in &ids {
            println!("MGET: {}_{}:{}", self.meta_namespace, namespace, id);
        }
        
        let res = cmd("MGET")
            .arg(
                ids
                    .iter()
                    .map(|x| format!("{}_{}:{}", self.meta_namespace, namespace, x))
                    .collect::<Vec<_>>(),
            )
            .query_async::<_, Vec<Option<R>>>(&mut redis_connection)
            .await?;
        Ok(res)
    }

    pub async fn delete<T1>(&self, namespace : &str, id : T1) -> Result<(), DatabaseError> 
    where T1 : Display
    {
        let mut redis_connection = self.pool.get().await?;

        cmd("DEL")
            .arg(format!("{}_{}:{}", self.meta_namespace, namespace, id))
            .query_async::<_, ()>(&mut redis_connection)
            .await?;

        Ok(())
    }

}

